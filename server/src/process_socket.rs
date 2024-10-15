use crate::client_handler::MCClient;
use crate::proxy_handler::ProxyClient;
use anyhow::{Context, Result};
use bytes::{BufMut, BytesMut};
use futures::SinkExt;
use shared::addressing::{DistributorError, Register};
use shared::config::PROXY_IDENTIFIER;
use shared::distributor_error;
use shared::minecraft::{MinecraftDataPacket, MinecraftHelloPacket};
use shared::packet_codec::PacketCodec;
use shared::socket_packet::SocketPacket;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
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
    socket
        .set_nodelay(true)
        .map_err(distributor_error!("could not set nodelay true"))?;
    let mut first_buf = [0u8; PROXY_IDENTIFIER.as_bytes().len()];
    socket.read_exact(&mut first_buf).await?;

    if first_buf != PROXY_IDENTIFIER.as_bytes() {
        let mut buf = BytesMut::new();
        let packet = async {
            buf.put(&first_buf[..]);
            socket.try_read_buf(&mut buf).unwrap();
            loop {
                if let Ok(packet) = MinecraftHelloPacket::new(&mut buf.clone()) {
                    break packet;
                }
                socket.read_buf(&mut buf).await.unwrap();
            }
        }
        .await;

        let proxy_tx = register.lock().await.servers.get(&packet.hostname).cloned();
        let proxy_tx = proxy_tx.ok_or(DistributorError::ServerNotFound(packet.hostname.clone()))?;

        let mut client = MCClient::new(
            proxy_tx.clone(),
            socket,
            packet,
            MinecraftDataPacket::from(buf.split().freeze()),
        )
        .await?;

        client.handle().await.context("Handler failed")?;
        return Ok(());
    }

    //println!("{:?}", buf);
    let mut frames = Framed::new(socket, PacketCodec::new(1024 * 8));
    // In a loop, read data from the socket and write the data back.

    // todo error handling
    let packet = frames.next().await.ok_or(DistributorError::UnknownError(
        "could not read first packet".to_string(),
    ))??;

    match packet {
        SocketPacket::ProxyHello(packet) => {
            tracing::info!(
                "Proxy client connected for {} from {}",
                packet.hostname,
                frames
                    .get_ref()
                    .peer_addr()
                    .map_err(distributor_error!("could not get peer addr"))?
            );
            let mut client = ProxyClient::new(register.clone(), &packet.hostname);
            // authenticate
            match timeout(
                Duration::from_secs(10),
                client.authenticate(&mut frames, &packet),
            )
            .await
            {
                Ok(Ok(client)) => client,
                Err(_) => {
                    frames
                        .send(SocketPacket::ProxyError("Timeout".into()))
                        .await?
                }
                Ok(Err(e)) => {
                    tracing::warn!("could not add proxy client: {}", e);
                    frames
                        .send(SocketPacket::ProxyError(format!(
                            "Error authenticating: {:?}",
                            e
                        )))
                        .await?;
                    return Err(e.into());
                }
            };
            if let Err(e) = client.register_connection().await {
                frames
                    .send(SocketPacket::ProxyError(
                        "Server already connected. Try again later!".to_string(),
                    ))
                    .await?;
                tracing::warn!("Server already connected!");
                return Err(e.into());
            }
            tracing::info!("Server {} registered and connected", packet.hostname);
            let response = client.handle(&mut frames).await;
            client.close_connection().await;
            response.context("proxy handler failed")?;
        }
        _ => {
            tracing::error!("Unknown protocol");
        }
    };

    Ok(())
}
