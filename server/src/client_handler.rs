use std::net::SocketAddr;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::codec::Framed;

use shared::addressing::{DistributorError, Tx};
use shared::distributor_error;
use shared::minecraft::{MinecraftDataPacket, MinecraftHelloPacket};
use shared::packet_codec::PacketCodec;
use shared::socket_packet::{ClientToProxy, SocketPacket};

#[derive(Debug)]
pub struct MCClient {
    frames: Framed<TcpStream, PacketCodec>,
    rx: UnboundedReceiver<MinecraftDataPacket>,
    addr: SocketAddr,
    proxy_tx: Tx,
    need_for_close: bool,
}

impl MCClient {
    /// Create a new instance of `Peer`.
    pub(crate) async fn new(
        proxy_tx: Tx,
        frames: Framed<TcpStream, PacketCodec>,
        hello_packet: MinecraftHelloPacket,
    ) -> Result<Self, DistributorError> {
        // Get the client socket address
        let addr = frames
            .get_ref()
            .peer_addr()
            .map_err(distributor_error!("could not get peer address"))?;
        let hostname = hello_packet.hostname;
        let (tx, rx) = mpsc::unbounded_channel();
        tracing::info!("sending client tx to proxy client {}", hostname);
        proxy_tx
            .send(ClientToProxy::AddMinecraftClient(addr, tx))
            .map_err(|_| {
                DistributorError::UnknownError("could not add minecraft client".to_string())
            })?;
        proxy_tx
            .send(ClientToProxy::Packet(
                addr,
                MinecraftDataPacket {
                    data: hello_packet.data,
                },
            ))
            .map_err(|_| {
                DistributorError::UnknownError("could not add minecraft client".to_string())
            })?;

        Ok(MCClient {
            frames,
            rx,
            proxy_tx,
            addr,
            need_for_close: true,
        })
    }
    /// HANDLE MC CLIENT
    pub async fn handle(&mut self) -> Result<(), DistributorError> {
        loop {
            tokio::select! {
                res = self.rx.recv() => {
                    match res {
                        Some(mut pkg) => {
                            loop {
                                let packet = SocketPacket::from(pkg);
                                if let Ok(pkg_next) = self.rx.try_recv() {
                                    self.frames.feed(packet).await.map_err(distributor_error!("could not feed packet"))?;
                                    pkg = pkg_next;
                                } else {
                                    self.frames.send(packet).await.map_err(distributor_error!("could not send packet"))?;
                                    break;
                                }
                            }
                        }
                        None => {
                            self.need_for_close = false;
                            tracing::info!("client channel closed by minecraft server {}", self.addr);
                            break
                        }
                    }
                }
                result = self.frames.next() => match result {
                    Some(Ok(SocketPacket::MCData(packet))) => {
                        if let Err(e) = self.proxy_tx.send(ClientToProxy::Packet(self.addr, packet)) {
                            tracing::error!("could not send to proxy distributor: {}", e);
                            break;
                        }
                    }
                    // An error occurred.
                    Some(Err(e)) => {
                        tracing::error!("Error while receiving: {:?}", e);
                        break;
                    }
                    // The stream has been exhausted.
                    None => break,
                    obj => {
                        tracing::error!("received unknown packet from client {:?}", obj);
                    }
                },
            }
        }
        Ok(())
    }
}

impl Drop for MCClient {
    fn drop(&mut self) {
        tracing::info!("dropping Client {}", self.addr);
        if self.need_for_close {
            let _ = self
                .proxy_tx
                .send(ClientToProxy::RemoveMinecraftClient(self.addr));
        }
    }
}
