use crate::client_handler::handle_minecraft_client;
use crate::proxy_handler::ProxyClient;
use crate::Register;
use anyhow::{Context, Result};
use futures::SinkExt;
use shared::addressing::DistributorError;
use shared::config::{PROXY_IDENTIFIER, TIMEOUT_IN_SEC};
use shared::packet_codec::PacketCodec;
use shared::proxy::ProxyHelloPacket;
use shared::socket_packet::{ClientToProxy, SocketPacket};
use std::collections::HashMap;
use std::future::Future;
use std::ops::Add;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep_until, Duration, Instant};
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

/// This function handles the connection to one client
/// it decides if the client is a minecraft client or a proxy client
/// forwards the traffic to the other side
/// encapsulates/encapsulates the packets
pub async fn process_socket_connection(mut socket: TcpStream, register: Register) -> Result<()> {
    socket.set_nodelay(true)?;
    let socket_start = Instant::now();

    let mut first_buf = [0u8; PROXY_IDENTIFIER.as_bytes().len()];
    if let Err(e) = timeout(&socket_start, socket.read_exact(&mut first_buf)).await {
        tracing::info!("Did not recognize protocol! Error: {e:?}");
        return Ok(());
    }
    // if the connection is a minecraft client
    if first_buf != PROXY_IDENTIFIER.as_bytes() {
        if let Err(e) = handle_minecraft_client(&first_buf, socket, register, &socket_start).await {
            tracing::error!("Error in client handler: {e:?}");
        }
        return Ok(());
    }

    // if the connection is a proxy client
    let proxy_client_version = timeout(&socket_start, socket.read_u16()).await?;

    let mut frames = Framed::new(socket, PacketCodec::default());

    // wait for a hello packet while permitting ping requests
    let hello_packet = timeout(&socket_start, wait_for_hello_packet(&mut frames)).await?;

    let mut client = ProxyClient::new(register.clone(), &hello_packet.hostname);

    // authenticate
    if let Err(e) = timeout(
        &socket_start,
        client.authenticate(&mut frames, &hello_packet),
    )
    .await
    {
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
    tracing::info!(
        "Server {} with version {} authorized and connected from {:?}",
        hello_packet.hostname,
        proxy_client_version,
        frames.get_ref().peer_addr()
    );
    let response = client.handle(&mut frames).await;
    client.close_connection().await;
    response.context("proxy handler failed")?;

    Ok(())
}

/// waits for a hello packet and returns it. If a Ping request is received, it gets responded
async fn wait_for_hello_packet(
    frames: &mut Framed<TcpStream, PacketCodec>,
) -> Result<ProxyHelloPacket, DistributorError> {
    loop {
        match frames.next().await {
            Some(Ok(SocketPacket::ProxyHello(packet))) => {
                return Ok(packet);
            }
            Some(Ok(SocketPacket::ProxyPing(ping))) => {
                frames.send(SocketPacket::ProxyPong(ping)).await?;
                continue;
            }
            e => {
                tracing::error!("Wrong first packet! {:?}", e);
                return Err(DistributorError::WrongPacket);
            }
        }
    }
}

pub async fn timeout<R, F, E>(start_time: &Instant, future: F) -> Result<R, DistributorError>
where
    E: Into<DistributorError>,
    F: Future<Output = Result<R, E>>,
{
    tokio::select! {
        res = future => res.map_err(|e|e.into()),
        _e = sleep_until(start_time.add(Duration::from_secs(TIMEOUT_IN_SEC))) => Err(DistributorError::Timeout)
    }
}
