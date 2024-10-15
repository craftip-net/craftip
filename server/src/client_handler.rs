use bytes::{Buf, BytesMut};
use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

use shared::addressing::{DistributorError, Tx};
use shared::distributor_error;
use shared::minecraft::{MinecraftDataPacket, MinecraftHelloPacket};
use shared::socket_packet::ClientToProxy;

#[derive(Debug)]
pub struct MCClient {
    socket: TcpStream,
    rx: UnboundedReceiver<MinecraftDataPacket>,
    addr: SocketAddr,
    proxy_tx: Tx,
    need_for_close: bool,
}

impl MCClient {
    /// Create a new instance of `Peer`.
    pub(crate) async fn new(
        proxy_tx: Tx,
        socket: TcpStream,
        hello_packet: MinecraftHelloPacket,
        start_data: MinecraftDataPacket,
    ) -> Result<Self, DistributorError> {
        // Get the client socket address
        let addr = socket
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
            .send(ClientToProxy::Packet(addr, start_data))
            .map_err(|_| {
                DistributorError::UnknownError("could not add minecraft client".to_string())
            })?;

        Ok(MCClient {
            socket,
            rx,
            proxy_tx,
            addr,
            need_for_close: true,
        })
    }
    /// HANDLE MC CLIENT
    pub async fn handle(&mut self) -> Result<(), DistributorError> {
        loop {
            let mut buf = BytesMut::new();
            tokio::select! {
                res = self.rx.recv() => {
                    match res {
                        Some(mut pkg) => {
                            loop {
                                if let Ok(pkg_next) = self.rx.try_recv() {
                                    self.socket.write_all(pkg.as_ref()).await.map_err(distributor_error!("could not feed packet"))?;
                                    pkg = pkg_next;
                                } else {
                                    self.socket.write_all(pkg.as_ref()).await.map_err(distributor_error!("could not send packet"))?;
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
                len = self.socket.read_buf(&mut buf) => match len {
                    // The stream has been exhausted.
                    Ok(0) => break,
                    Ok(len) => {
                        let packet = MinecraftDataPacket::from(buf.split().freeze());

                        if let Err(e) = self.proxy_tx.send(ClientToProxy::Packet(self.addr, packet)) {
                            tracing::error!("could not send to proxy distributor: {}", e);
                            break;
                        }
                    }
                    // An error occurred.
                    Err(e) => {
                        tracing::error!("Error while receiving: {:?}", e);
                        break;
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
