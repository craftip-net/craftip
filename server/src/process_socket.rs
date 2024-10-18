use crate::client_handler::{first_minecraft_packet, MCClient};
use crate::disconnect_client::handle_mc_disconnect;
use crate::proxy_handler::ProxyClient;
use anyhow::{Context, Result};
use futures::SinkExt;
use shared::addressing::{DistributorError, Register};
use shared::config::{PROXY_IDENTIFIER, TIMEOUT_IN_SEC};
use shared::packet_codec::PacketCodec;
use shared::socket_packet::SocketPacket;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

/// This function handles the connection to one client
/// it decides if the client is a minecraft client or a proxy client
/// forwards the traffic to the other side
/// encapsulates/encapsulates the packets
pub async fn process_socket_connection(
    mut socket: TcpStream,
    register: Arc<Mutex<Register>>,
) -> Result<()> {
    socket.set_nodelay(true)?;

    let mut first_buf = [0u8; PROXY_IDENTIFIER.as_bytes().len()];
    if let Err(e) = timeout(socket.read_exact(&mut first_buf)).await {
        tracing::debug!("Error while connecting to socket {:?}", e);
        return Ok(());
    }

    if first_buf != PROXY_IDENTIFIER.as_bytes() {
        let (packet, packet_data) =
            match timeout(first_minecraft_packet(&mut socket, &first_buf)).await {
                Err(e) => {
                    tracing::debug!("Connection incomplete {:?}", e);
                    return Ok(());
                }
                Ok(e) => e,
            };

        let proxy_tx = register.lock().await.servers.get(&packet.hostname).cloned();
        if proxy_tx.is_none() {
            // if there is no proxy connected for the corresponding server
            let _ = tokio::time::timeout(
                Duration::from_secs(TIMEOUT_IN_SEC),
                handle_mc_disconnect(packet, packet_data, socket),
            )
            .await;
            return Ok(());
        }
        let proxy_tx = proxy_tx.ok_or(DistributorError::ServerNotFound(packet.hostname.clone()))?;

        let mut client = MCClient::new(proxy_tx.clone(), socket, packet, packet_data).await?;

        return client
            .handle()
            .await
            .map_err(|e| {
                tracing::error!("{:?}", e);
                e
            })
            .map_err(Into::into);
    }

    let proxy_client_version = timeout(socket.read_u16()).await?;

    let mut frames = Framed::new(socket, PacketCodec::new(1024 * 8));
    // In a loop, read data from the socket and write the data back.

    let hello_packet = match frames.next().await {
        Some(Ok(SocketPacket::ProxyHello(packet))) => {
            tracing::info!(
                "Proxy client v{} connected for {} from {:?}",
                proxy_client_version,
                packet.hostname,
                frames.get_ref().peer_addr()
            );
            packet
        }
        e => {
            tracing::info!("Wrong first packet! {:?}", e);
            return Ok(());
        }
    };

    let mut client = ProxyClient::new(register.clone(), &hello_packet.hostname);

    // authenticate
    if let Err(e) = timeout(client.authenticate(&mut frames, &hello_packet)).await {
        tracing::warn!("could not add proxy client: {:?}", e);
        let e = SocketPacket::ProxyError(format!("Error authenticating: {:?}", e));
        frames.send(e).await?;
        return Ok(());
    }
    if let Err(_err) = client.register_connection().await {
        let p = SocketPacket::ProxyError("Server already connected. Try again later!".to_string());
        frames.send(p).await?;
        tracing::info!("Server {} already connected!", hello_packet.hostname);
        return Ok(());
    }
    tracing::debug!("Server {} registered and connected", hello_packet.hostname);
    let response = client.handle(&mut frames).await;
    client.close_connection().await;
    response.context("proxy handler failed")?;

    Ok(())
}

pub async fn timeout<R, F, E>(future: F) -> Result<R, DistributorError>
where
    E: Into<DistributorError>,
    F: Future<Output = Result<R, E>>,
{
    match tokio::time::timeout(Duration::from_secs(TIMEOUT_IN_SEC), future).await {
        Ok(result) => result.map_err(Into::into),
        Err(_) => Err(DistributorError::Timeout),
    }
}
