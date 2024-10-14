use crate::client_handler::MCClient;
use crate::proxy_handler::ProxyClient;
use futures::SinkExt;
use shared::addressing::{DistributorError, Register};
use shared::distributor_error;
use shared::packet_codec::PacketCodec;
use shared::socket_packet::SocketPacket;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_stream::StreamExt;
use tokio_util::codec::Framed;

/// This function handles the connection to one client
/// it decides if the client is a minecraft client or a proxy client
/// forwards the traffic to the other side
/// encapsulates/decapsulates the packets
pub async fn process_socket_connection(
    socket: TcpStream,
    register: Arc<Mutex<Register>>,
) -> Result<(), DistributorError> {
    socket
        .set_nodelay(true)
        .map_err(distributor_error!("could not set nodelay true"))?;
    let mut frames = Framed::new(socket, PacketCodec::new(1024 * 8));
    // In a loop, read data from the socket and write the data back.
    let packet = frames.next().await.ok_or(DistributorError::UnknownError(
        "could not read first packet".to_string(),
    ))?;
    let packet = packet.map_err(distributor_error!("could not read packet"))?;

    match packet {
        SocketPacket::MCHello(packet) => {
            let proxy_tx = register.lock().await.servers.get(&packet.hostname).cloned();
            let proxy_tx =
                proxy_tx.ok_or(DistributorError::ServerNotFound(packet.hostname.clone()))?;

            let mut client = MCClient::new(proxy_tx.clone(), frames, packet).await?;

            client.handle().await?;
        }
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
                    return Err(e);
                }
            };
            if let Err(e) = client.register_connection().await {
                frames
                    .send(SocketPacket::ProxyError(
                        "Server already connected. Try again later!".to_string(),
                    ))
                    .await?;
                tracing::warn!("Server already connected!");
                return Err(e);
            }
            let response = client.handle(&mut frames).await;
            client.close_connection().await;
            response?;
        }
        _ => {
            tracing::error!("Unknown protocol");
        }
    };

    Ok(())
}
