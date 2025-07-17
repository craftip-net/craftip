use crate::client_handler::handle_minecraft_client;
use crate::proxy_handler::ProxyClient;
use crate::Register;
use anyhow::Result;
use shared::addressing::DistributorError;
use shared::config::{PROXY_IDENTIFIER, TIMEOUT_IN_SEC};
use shared::packet_codec::PacketCodec;
use std::future::Future;
use std::ops::Add;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::{sleep, sleep_until, Duration, Instant};
use tokio_util::codec::Framed;

/// This function handles the connection to one client
/// it decides if the client is a minecraft client or a proxy client
/// forwards the traffic to the other side
/// encapsulates/encapsulates the packets
pub async fn process_socket_connection(mut socket: TcpStream, register: Register) {
    let socket_start = Instant::now();
    if let Err(e) = socket.set_nodelay(true) {
        tracing::error!("could not set no_delay(true) {e:?}");
    }

    let Ok(ip) = socket.peer_addr() else {
        tracing::error!("Could not get socket address");
        return;
    };

    // detecting what kind of protocol it is
    let mut first_buf = [0u8; PROXY_IDENTIFIER.as_bytes().len()];
    loop {
        match timeout(&socket_start, socket.peek(&mut first_buf)).await {
            Ok(len) => {
                if len == PROXY_IDENTIFIER.as_bytes().len() {
                    break;
                }
            }
            Err(e) => {
                tracing::info!("Did not recognize protocol! Error: {e:?} of {ip:?}");
                return;
            }
        }
        sleep(Duration::from_millis(500)).await;
    }

    // if the connection is a minecraft client
    if first_buf != PROXY_IDENTIFIER.as_bytes() {
        if let Err(e) = handle_minecraft_client(socket, register, &socket_start).await {
            tracing::error!("Error in client handler: {e:?}");
        }
        return;
    }

    // remove leading magic numbers
    let ident = &mut [0u8; PROXY_IDENTIFIER.len()];
    let Ok(_) = timeout(&socket_start, socket.read_exact(ident)).await else {
        return;
    };
    let Ok(_proxy_version) = timeout(&socket_start, socket.read_u16()).await else {
        return;
    };

    let frames = Framed::new(socket, PacketCodec::default());

    // wait for a hello packet while permitting ping requests
    let Ok(proxy) = timeout(&socket_start, ProxyClient::new(register.clone(), frames)).await else {
        return;
    };

    // authenticate
    let proxy = match timeout(&socket_start, proxy.authenticate()).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("could not add proxy client: {:?}", e);
            return;
        }
    };

    proxy.handle(&ip).await;
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
