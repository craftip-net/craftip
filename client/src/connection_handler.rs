use anyhow::{Context, Result};
use bytes::BytesMut;
use shared::minecraft::MinecraftDataPacket;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use crate::structs::{ClientToProxy, ClientToProxyTx, ProxyToClientRx, ProxyToClientTx};
use shared::socket_packet::{ClientID, SocketPacket};

pub type Tx = UnboundedSender<Option<SocketPacket>>;
pub struct ClientConnection {
    mc_server: String,
    client_id: ClientID,
    client_rx: ProxyToClientRx,
    proxy_tx: ClientToProxyTx,
    pub need_for_close: bool,
}

impl ClientConnection {
    pub async fn new(
        proxy_tx: ClientToProxyTx,
        mc_server: String,
        client_id: ClientID,
    ) -> (Self, ProxyToClientTx) {
        let (client_tx, client_rx) = unbounded_channel();
        (
            Self {
                mc_server,
                client_id,
                client_rx,
                proxy_tx,
                need_for_close: true,
            },
            client_tx,
        )
    }
    pub async fn handle_client(&mut self) -> Result<()> {
        tracing::info!("opening new client with id {}", self.client_id);
        // connect to server
        let mut mc_server = TcpStream::connect(&self.mc_server)
            .await
            .context(format!("could not connect to {}", &self.mc_server))?;
        mc_server.set_nodelay(true)?;
        loop {
            let mut buf = BytesMut::new();
            tokio::select! {
                pkg = self.client_rx.recv() => {
                    //tracing::info!("Sending packet to client: {:?}", pkg);
                    match pkg {
                        Some(packet) => {
                            if let Err(err) = mc_server.write_all(packet.as_ref()).await {
                                tracing::error!("write_all failed: {}", err);
                                break;
                            }
                        }
                        None => {
                            self.need_for_close = false;
                            return Ok(())
                        }
                    }
                }
                n = mc_server.read_buf(&mut buf) => {
                    let n = match n {
                        Ok(n) => n,
                        Err(err) => {
                            tracing::error!("read failed: {}", err);
                            break;
                        }
                    };
                    if n == 0 {
                        tracing::info!("Minecraft server closed connection!");
                        break;
                    }
                    tracing::debug!("recv pkg from mc srv len: {}", n);
                    // encapsulate in ProxyDataPacket
                    let packet = MinecraftDataPacket::from(buf.split().freeze());
                    let packet = ClientToProxy::Packet(self.client_id,  packet);

                    if let Err(e) = self.proxy_tx.send(packet) {
                        tracing::error!("tx send failed: {}", e);
                        break;
                    }
                }
            }
        }
        tracing::trace!("closing client connection");
        self.need_for_close = true;
        Ok(())
    }
    /// Sends a disconnect packet to the proxy server
    pub async fn close(&self) {
        // if this fails, channel is already closed. Therefore, not important
        let _ = self
            .proxy_tx
            .send(ClientToProxy::RemoveMinecraftClient(self.client_id));
    }
    pub fn set_death(&self, error: String) {
        let _ = self.proxy_tx.send(ClientToProxy::Death(error));
    }
}

impl Drop for ClientConnection {
    fn drop(&mut self) {
        tracing::debug!("dropping client connection {}", self.client_id);
        if self.need_for_close {
            let _ = self
                .proxy_tx
                .send(ClientToProxy::RemoveMinecraftClient(self.client_id));
        }
    }
}
