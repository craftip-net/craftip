use crate::client_handler::handle_minecraft_client;
use crate::proxy_handler::ProxyClient;
use crate::Register;
use anyhow::{Context, Result};
use futures::SinkExt;
use shared::addressing::DistributorError;
use shared::config::{PROXY_IDENTIFIER, TIMEOUT_IN_SEC};
use shared::packet_codec::PacketCodec;
use shared::proxy::ProxyHelloPacket;
use shared::socket_packet::SocketPacket;
use std::future::Future;
use std::ops::Add;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
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
    let ip = socket.peer_addr()?;

    let mut first_buf = [0u8; PROXY_IDENTIFIER.as_bytes().len()];
    if let Err(e) = timeout(&socket_start, socket.read_exact(&mut first_buf)).await {
        let ip = socket.peer_addr();
        tracing::info!("Did not recognize protocol! Error: {e:?} of {ip:?}");
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
    let _proxy_client_version = timeout(&socket_start, socket.read_u16()).await?;

    let mut frames = Framed::new(socket, PacketCodec::default());

    // wait for a hello packet while permitting ping requests
    let hello = timeout(&socket_start, wait_for_hello_packet(&mut frames)).await?;

    let mut client = ProxyClient::new(register.clone(), &hello.hostname);

    // authenticate
    if let Err(e) = timeout(&socket_start, client.authenticate(&mut frames, &hello)).await {
        tracing::warn!("could not add proxy client: {:?}", e);
        let e = SocketPacket::ProxyError(format!("Error authenticating: {:?}", e));
        frames.send(e).await?;
        return Ok(());
    }

    client.handle(frames, ip).await;
    // important! removes proxy from register
    client.cleanup().await;

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
