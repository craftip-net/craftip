use std::sync::Arc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use shared::addressing::Tx;
use shared::socket_packet::{ChannelMessage, SocketPacket};
use anyhow::{Result, Context};
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use shared::proxy::{ProxyClientDisconnectPacket, ProxyDataPacket};

use crate::client::Shared;

pub struct ClientConnection {
    mc_server: String,
    client_id: u16,
    client_rx: UnboundedReceiver<ChannelMessage<Vec<u8>>>,
    proxy_tx: Tx,
    state: Arc<Mutex<Shared>>,
}
impl ClientConnection {
    pub async fn new(state: Arc<Mutex<Shared>>, proxy_tx: Tx, mc_server: String, client_id: u16) -> Self {
        let (client_tx, client_rx) = unbounded_channel();
        state.lock().await.add_connection(client_id, client_tx);
        Self {
            mc_server,
            client_id,
            client_rx,
            proxy_tx,
            state,
        }
    }
    pub async fn handle_client(&mut self) -> Result<()> {
        tracing::info!("opening new client with id {}", self.client_id);
        // connect to server
        let mut buf = [0; 1024];
        let mut mc_server = TcpStream::connect(&self.mc_server).await
            .context(format!("could not connect to {}", &self.mc_server))?;
        loop {
            tokio::select! {
                Some(pkg) = self.client_rx.recv() => {
                    //tracing::info!("Sending packet to client: {:?}", pkg);
                    match pkg {
                        ChannelMessage::Packet(data) => {
                            if let Err(err) = mc_server.write_all(&data).await {
                                tracing::error!("write_all failed: {}", err);
                                break;
                            }
                        }
                        ChannelMessage::Close => {
                            break;
                        }
                    }
                }
                n = mc_server.read(&mut buf) => {
                    let n = match n {
                        Ok(n) => n,
                        Err(err) => {
                            tracing::error!("read failed: {}", err);
                            break;
                        }
                    };
                    if n == 0 {
                        tracing::info!("Minecraft server closed connection!");
                        break; // server 2 has closed the connection
                    }
                    tracing::debug!("recv pkg from mc srv len: {}", n);
                    // encapsulate in ProxyDataPacket
                    let packet = SocketPacket::from(ProxyDataPacket::new(buf[0..n].to_vec(), n, self.client_id));

                    if let Err(e) = self.proxy_tx.send(ChannelMessage::Packet(packet)) {
                        tracing::error!("tx send failed: {}", e);
                        break;
                    }
                }
            }
        }
        tracing::trace!("closing client connection");

        Ok(())
    }
    pub async fn close(&self) {
        let disconnect_pkg = SocketPacket::from(ProxyClientDisconnectPacket::new(self.client_id));
        // if this fails, channel is already closed. Therefore not important
        let _ = self.proxy_tx.send(ChannelMessage::Packet(disconnect_pkg));
        self.state.lock().await.remove_connection(self.client_id);
    }
}