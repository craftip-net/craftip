use std::mem;
use std::mem::size_of;
use std::ops::{Add, Sub};

use anyhow::{bail, Result};
use futures::SinkExt;
use shared::addressing::DistributorError;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, timeout, Duration, Instant};
use tokio_stream::StreamExt;
use tokio_util::bytes::{BufMut, BytesMut};
use tokio_util::codec::Framed;

use shared::config::{MAXIMUM_CLIENTS, PROTOCOL_VERSION, PROXY_IDENTIFIER, TIMEOUT_IN_SEC};
use shared::packet_codec::PacketCodec;
use shared::proxy::{ProxyAuthenticator, ProxyDataPacket, ProxyHelloPacket};
use shared::socket_packet::{ClientID, SocketPacket};

use crate::connection_handler::ClientConnection;
use crate::structs::{
    ClientError, ClientToProxy, ProxyToClient, ProxyToClientTx, Server, ServerAuthentication,
    Stats, StatsTx,
};

pub struct Client {
    connections: [Option<ProxyToClientTx>; MAXIMUM_CLIENTS],
    connections_len: usize,
    stats_tx: Option<StatsTx>,
    proxy: Option<Framed<TcpStream, PacketCodec>>,
    server: Server,
}

impl Client {
    fn add_connection(&mut self, id: ClientID, tx: ProxyToClientTx) {
        if let Some(e) = self.connections.get_mut(id) {
            *e = Some(tx);
            self.connections_len = self.connections_len.saturating_add(1);
        }
        if let Some(tx) = &self.stats_tx {
            tx.send(Stats::ClientsConnected(self.connections_len))
                .unwrap();
        }
    }
    pub fn remove_connection(&mut self, id: ClientID) {
        if let Some(e) = self.connections.get_mut(id) {
            if e.is_none() {
                return;
            }
            *e = None;
            self.connections_len = self.connections_len.saturating_sub(1);
        }
        if let Some(tx) = &self.stats_tx {
            tx.send(Stats::ClientsConnected(self.connections_len))
                .unwrap();
        }
    }
    pub fn send_to(&mut self, id: ClientID, msg: ProxyToClient) -> Result<(), DistributorError> {
        let channel = self
            .connections
            .get_mut(id)
            .and_then(|inner| inner.as_ref())
            .ok_or(DistributorError::ClientNotFound)?;
        if let Err(e) = channel.send(msg) {
            tracing::error!("Distributor: Send error {e} removing {id}...");
            self.remove_connection(id);
        }
        Ok(())
    }
}

impl Client {
    pub fn new(server: Server, stats_tx: StatsTx) -> Self {
        const CONNECTION_NONE: Option<ProxyToClientTx> = None;
        Client {
            connections: [CONNECTION_NONE; MAXIMUM_CLIENTS],
            connections_len: 0,
            server,
            stats_tx: Some(stats_tx),
            proxy: None,
        }
    }
}

impl Client {
    pub async fn connect(&mut self) -> Result<(), ClientError> {
        // test connection to minecraft server
        timeout(
            Duration::from_secs(5),
            TcpStream::connect(&self.server.local),
        )
        .await
        .map_err(|_| ClientError::MinecraftServerNotFound)?
        .map_err(|_| ClientError::MinecraftServerNotFound)?;
        // connect to proxy
        let mut proxy_stream = TcpStream::connect(format!("{}:25565", &self.server.server)).await?;
        proxy_stream.set_nodelay(true)?;

        // identifying as proxy
        let mut buf = BytesMut::with_capacity(PROXY_IDENTIFIER.len() + size_of::<u16>());
        buf.put_slice(PROXY_IDENTIFIER.as_bytes());
        buf.put_u16(PROTOCOL_VERSION);
        // writing version
        proxy_stream.write_all(&buf).await?;

        let proxy = Framed::new(proxy_stream, PacketCodec::default());

        self.proxy = Some(proxy);
        Ok(())
    }

    pub async fn auth(&mut self) -> Result<(), ClientError> {
        let proxy = self.proxy.as_mut().unwrap();
        let hello = SocketPacket::from(ProxyHelloPacket {
            version: PROTOCOL_VERSION,
            hostname: self.server.server.clone(),
            auth: match &mut self.server.auth {
                ServerAuthentication::Key(private_key) => {
                    ProxyAuthenticator::PublicKey(private_key.get_public_key())
                }
            },
        });

        proxy.send(hello).await?;
        let challenge = match proxy.next().await {
            None => return Err(ClientError::ProxyClosedConnection),
            Some(Ok(SocketPacket::ProxyAuthRequest(pkg))) => pkg,
            Some(e) => return Err(ClientError::UnexpectedPacket(format!("{:?}", e))),
        };

        match &mut self.server.auth {
            ServerAuthentication::Key(private_key) => {
                let signature = private_key.sign(&challenge);
                proxy
                    .send(SocketPacket::ProxyAuthResponse(signature))
                    .await?;
            }
        }

        match proxy.next().await {
            Some(Ok(SocketPacket::ProxyHelloResponse(_hello_response))) => Ok(()),
            Some(Ok(SocketPacket::ProxyError(e))) => Err(ClientError::ProxyError(e)),
            None => Err(ClientError::ProxyClosedConnection),
            Some(Err(e)) => Err(ClientError::ProtocolError(e)),
            e => return Err(ClientError::UnexpectedPacket(format!("{:?}", e))),
        }?;

        tracing::info!("Connected to proxy server!");

        if let Some(stats) = &self.stats_tx {
            stats
                .send(Stats::Connected)
                .map_err(|e| ClientError::Other(e.into()))?;
        }
        Ok(())
    }

    pub async fn ping(&mut self) -> Result<u128, ClientError> {
        let start = Instant::now();
        let proxy = self.proxy.as_mut().unwrap();
        let random = start.elapsed().as_nanos() as u16;
        proxy.send(SocketPacket::ProxyPing(random)).await?;
        let random_ret = match proxy.next().await {
            Some(Ok(SocketPacket::ProxyPong(random))) => random,
            other => {
                return Err(ClientError::UnexpectedPacket(format!(
                    "Waited for pong but got {:?}",
                    other
                )))
            }
        };

        if random != random_ret {
            return Err(ClientError::UnexpectedPacket(format!(
                "Did not get same time back {} instead of {}",
                random_ret, random
            )));
        }

        Ok(start.elapsed().as_millis())
    }

    pub async fn handle(&mut self) -> Result<()> {
        let (to_proxy_tx, mut to_proxy_rx) = mpsc::unbounded_channel();
        let mut proxy = mem::take(&mut self.proxy).unwrap();
        let mut last_packet_recv = Instant::now();
        let mut last_ping_recv = Instant::now().sub(Duration::from_secs(10));
        let start = Instant::now();
        loop {
            tokio::select! {
                // send packets to proxy
                Some(mut pkg) = to_proxy_rx.recv() => {
                    // doing this in a loop to only feed the socket if there are more packets pending to be sent
                    loop {
                        let socket_packet = match pkg {
                            ClientToProxy::Packet(id, pkg) => {
                                SocketPacket::from(ProxyDataPacket::new(pkg, id))
                            },
                            ClientToProxy::RemoveMinecraftClient(id) => {
                                self.remove_connection(id);
                                SocketPacket::ProxyDisconnect(id)
                            },
                            ClientToProxy::Death(msg) => {
                                bail!(msg);
                            }
                        };
                        // If channel is not exhausted, just feed the socket without sending
                        if let Ok(pkg_next) = to_proxy_rx.try_recv() {
                            proxy.feed(socket_packet).await?;
                            pkg = pkg_next;
                        } else {
                            // send and therefore flush socket on last socket in the channel
                            proxy.send(socket_packet).await?;
                            break;
                        }
                    }
                }

                // receive proxy packets
                result = proxy.next() => {
                    last_packet_recv = Instant::now();
                    match result {
                        Some(Ok(msg)) => {
                            match msg {
                                SocketPacket::ProxyJoin(client_id) => {
                                    let (mut client_connection, client_tx) = ClientConnection::new(to_proxy_tx.clone(), self.server.local.clone(), client_id).await;
                                    self.add_connection(client_id, client_tx);
                                    tokio::spawn(async move {
                                        if let Err(e) = client_connection.handle_client().await {
                                            tracing::error!("An Error occurred in the handle_client function: {}", e);
                                            // sometimes handle_client closes after gui, errors can occur
                                            client_connection.set_death(e.to_string());
                                        }
                                    });
                                }
                                SocketPacket::ProxyData(packet) => {
                                    self.send_to(packet.client_id, packet.data)?;
                                }
                                SocketPacket::ProxyDisconnect(client_id) => {
                                    // this can fail if the client is already disconnected
                                    self.remove_connection(client_id);
                                }
                                SocketPacket::ProxyPong(ping) => {
                                    let time = start.elapsed().as_millis() as u16;
                                    let ping = time.saturating_sub(ping);
                                    if let Some(stats) = &self.stats_tx {
                                        stats.send(Stats::Ping(ping))?;
                                    }
                                }
                                e => bail!("Unexpected message {:?}", e)
                            }
                        }
                        // An error occurred.
                        Some(Err(e)) => bail!("an error occurred while processing messages error = {:?}", e),
                        // The stream has been exhausted.
                        None => bail!("Proxy has closed the connection")
                    }
                },
                // ensure constant traffic so tcp connection does not close
                _ = sleep_until(last_ping_recv.add(Duration::from_secs(5))) => {
                    last_ping_recv = Instant::now();
                    let time = start.elapsed().as_millis() as u16;
                    proxy.send(SocketPacket::ProxyPing(time)).await?;
                }
                // terminate socket if TIMEOUT_IN_SEC no packet was received
                 _ = sleep_until(last_packet_recv.add(Duration::from_secs(TIMEOUT_IN_SEC))) => {
                    tracing::error!("socket timed out");
                    bail!("Connection timed out!");
                }
            }
        }
    }
}
