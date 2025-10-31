use std::mem;
use std::mem::size_of;
use std::ops::{Add, Sub};

use anyhow::{bail, Result};
use futures::{StreamExt, SinkExt};
use tokio::io::AsyncWriteExt;
use shared::addressing::DistributorError;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::time::{sleep_until, timeout, Duration, Instant};
use tokio_util::bytes::{BufMut, BytesMut};
use tokio_util::codec::{FramedRead, FramedWrite};

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
    proxy: Option<(FramedRead<OwnedReadHalf, PacketCodec>, FramedWrite<OwnedWriteHalf, PacketCodec>)>,
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

        let (reader, writer) = proxy_stream.into_split();
        let reader = FramedRead::new(reader, PacketCodec::default());
        let writer = FramedWrite::new(writer, PacketCodec::default());

        self.proxy = Some((reader, writer));
        Ok(())
    }

    pub async fn auth(&mut self) -> Result<(), ClientError> {
        let (reader, writer) = self.proxy.as_mut().unwrap();
        let hello = SocketPacket::from(ProxyHelloPacket {
            version: PROTOCOL_VERSION,
            hostname: self.server.server.clone(),
            auth: match &mut self.server.auth {
                ServerAuthentication::Key(private_key) => {
                    ProxyAuthenticator::PublicKey(private_key.get_public_key())
                }
            },
        });

        writer.send(hello).await?;
        let challenge = match reader.next().await {
            None => return Err(ClientError::ProxyClosedConnection),
            Some(Ok(SocketPacket::ProxyAuthRequest(pkg))) => pkg,
            Some(e) => return Err(ClientError::UnexpectedPacket(format!("{:?}", e))),
        };

        match &mut self.server.auth {
            ServerAuthentication::Key(private_key) => {
                let signature = private_key.sign(&challenge);
                writer
                    .send(SocketPacket::ProxyAuthResponse(signature))
                    .await?;
            }
        }

        match reader.next().await {
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
        let (reader, writer) = self.proxy.as_mut().unwrap();
        let random = start.elapsed().as_nanos() as u16;
        writer.send(SocketPacket::ProxyPing(random)).await?;
        let random_ret = match reader.next().await {
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

    async fn writer_handler(
        mut to_proxy_rx: UnboundedReceiver<ClientToProxy>,
        mut writer: FramedWrite<OwnedWriteHalf, PacketCodec>,
        disconnect_tx: UnboundedSender<ClientID>,
    ) -> Result<()> {
        writer.set_backpressure_boundary(128 * 1024);
        loop {
            // only flush if to_proxy_rx is empty
            let pkg = match to_proxy_rx.try_recv() {
                Ok(pkg) => pkg,
                Err(_) => {
                    // flush writer before waiting for more messages
                    if let Err(e) = SinkExt::<SocketPacket>::flush(&mut writer).await {
                        tracing::warn!("Failed to flush packet: {}", e);
                        return Ok(());
                    }
                    match to_proxy_rx.recv().await {
                        Some(pkg) => pkg,
                        None => return Ok(()),
                    }
                }
            };

            match pkg {
                ClientToProxy::Packet(id, pkg) => {
                    writer.feed(SocketPacket::from(ProxyDataPacket::new(pkg, id))).await?
                }
                ClientToProxy::RemoveMinecraftClient(id) => {
                    if let Err(e) = disconnect_tx.send(id) {
                        tracing::warn!("Reader task ended? {:?}", e);
                        return Ok(());
                    }

                    writer.feed(SocketPacket::ProxyDisconnect(id)).await?;
                }
                ClientToProxy::Death(msg) => {
                    tracing::error!("Received a dead client: {}", msg);
                    return Ok(());
                }
                ClientToProxy::Ping(ping) => writer.feed(SocketPacket::ProxyPing(ping)).await?,
            };
        }
    }

    async fn reader_handler(
        &mut self,
        mut reader: FramedRead<OwnedReadHalf, PacketCodec>,
        mut disconnect_rx: UnboundedReceiver<ClientID>,
        to_proxy_tx: UnboundedSender<ClientToProxy>,
    ) -> Result<()> {
        let mut last_packet_recv = Instant::now();
        let mut last_ping_recv = Instant::now().sub(Duration::from_secs(10));
        let start = Instant::now();
        loop {
            tokio::select! {
                // receive proxy packets
                result = reader.next() => {
                    last_packet_recv = Instant::now();
                    let msg = match result {
                        Some(Ok(msg)) => msg,
                        // An error occurred.
                        Some(Err(e)) => bail!("an error occurred while processing messages error = {:?}", e),
                        // The stream has been exhausted.
                        None => bail!("Proxy has closed the connection")
                    };
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
                },
                res = disconnect_rx.recv() => {
                    match res {
                        Some(id) => self.remove_connection(id),
                        None => {
                            tracing::error!("client handler writer failed. Exiting");
                            return Ok(())
                        },
                    }
                },
                // ensure constant traffic so tcp connection does not close
                _ = sleep_until(last_ping_recv.add(Duration::from_secs(5))) => {
                    last_ping_recv = Instant::now();
                    let time = start.elapsed().as_millis() as u16;
                    //proxy.send(SocketPacket::ProxyPing(time)).await?;
                    to_proxy_tx.send(ClientToProxy::Ping(time))?;
                }
                // terminate socket if TIMEOUT_IN_SEC no packet was received
                 _ = sleep_until(last_packet_recv.add(Duration::from_secs(TIMEOUT_IN_SEC))) => {
                    tracing::error!("socket timed out");
                    bail!("Connection timed out!");
                }
            }
        }
    }

    pub async fn handle(&mut self) -> Result<()> {
        let (to_proxy_tx, to_proxy_rx) = mpsc::unbounded_channel();
        let (reader, writer) = mem::take(&mut self.proxy).unwrap();
        let (disconnect_tx, disconnect_rx) = unbounded_channel::<ClientID>();

        let writer_task = tokio::spawn(Self::writer_handler(to_proxy_rx, writer, disconnect_tx));
        let res = self
            .reader_handler(reader, disconnect_rx, to_proxy_tx)
            .await;
        writer_task.abort();

        res
    }
}
