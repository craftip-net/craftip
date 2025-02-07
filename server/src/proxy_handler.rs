use std::ops::Add;
use std::sync::Arc;

use futures::future::select;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::task;
use tokio::time::sleep_until;
use tokio::time::{Duration, Instant};
use tokio_util::codec::Framed;

use shared::addressing::DistributorError;
use shared::config::{MAXIMUM_CLIENTS, PROTOCOL_VERSION, TIMEOUT_IN_SEC};
use shared::minecraft::MinecraftDataPacket;
use shared::packet_codec::PacketCodec;
use shared::proxy::{
    ProxyAuthenticator, ProxyConnectedResponse, ProxyDataPacket, ProxyHelloPacket,
};
use shared::socket_packet::{ClientID, PingPacket, SocketPacket};

use crate::register::Register;

#[derive(Debug)]
pub struct Distribiutor {
    clients_id: [Option<UnboundedSender<MinecraftDataPacket>>; MAXIMUM_CLIENTS],
}

/// Custom packet type for tokio channels to be able to close the client socket by the proxy
/// uses Packet type as a generic type
/// or Close to close the socket
#[derive(Debug)]
pub enum ClientToProxy {
    Packet(ClientID, MinecraftDataPacket),
    AddMinecraftClient(
        oneshot::Sender<ClientID>,
        UnboundedSender<MinecraftDataPacket>,
    ),
    RemoveMinecraftClient(ClientID),
    AnswerPingPacket(PingPacket),
}

impl Default for Distribiutor {
    fn default() -> Self {
        const CHANNEL_NONE: Option<UnboundedSender<MinecraftDataPacket>> = None;
        Self {
            clients_id: [CHANNEL_NONE; MAXIMUM_CLIENTS],
        }
    }
}
impl Distribiutor {
    fn insert(
        &mut self,
        tx: UnboundedSender<MinecraftDataPacket>,
    ) -> Result<ClientID, DistributorError> {
        for (id, element) in self.clients_id.iter_mut().enumerate() {
            if element.is_none() {
                *element = Some(tx);
                return Ok(id);
            }
        }
        Err(DistributorError::TooManyClients)
    }
    fn remove_by_id(&mut self, id: ClientID) {
        if let Some(client) = self.clients_id.get_mut(id) {
            client.take();
        }
    }

    fn get_by_id(&self, id: ClientID) -> Option<&UnboundedSender<MinecraftDataPacket>> {
        let sender = self.clients_id.get(id);
        return sender.and_then(|inner| inner.as_ref());
    }
}

#[derive(Debug)]
pub struct ProxyClient {
    register: Register,
    hostname: String,
    rx: Option<UnboundedReceiver<ClientToProxy>>,
    tx: Option<UnboundedSender<ClientToProxy>>,
    connected_time: Option<Instant>,
}

impl ProxyClient {
    pub fn new(register: Register, hostname: &str) -> Self {
        ProxyClient {
            register,
            hostname: hostname.to_string(),
            rx: None,
            tx: None,
            connected_time: None,
        }
    }
    /// HANDLE PROXY CLIENT
    pub async fn handle(
        &mut self,
        mut framed: Framed<TcpStream, PacketCodec>,
    ) -> Result<(), DistributorError> {
        // send connected
        let resp = SocketPacket::from(ProxyConnectedResponse {
            version: PROTOCOL_VERSION,
        });
        framed.send(resp).await?;
        self.connected_time = Some(Instant::now());
        let rx = self.rx.take().unwrap();
        let tx = self.tx.take().unwrap();
        let distributor = Arc::new(Mutex::new(Distribiutor::default()));

        let (writer, reader) = framed.split::<SocketPacket>();

        let reader = task::spawn(ProxyClient::handle_reader(reader, distributor.clone(), tx));
        let writer = task::spawn(ProxyClient::handle_writer(writer, distributor.clone(), rx));
        let _res = select(reader, writer).await;
        // terminate the other task?
        //res.factor_second().1.abort();

        Ok(())
    }

    async fn handle_reader(
        mut reader: SplitStream<Framed<TcpStream, PacketCodec>>,
        distributor: Arc<Mutex<Distribiutor>>,
        tx: UnboundedSender<ClientToProxy>,
    ) {
        let mut last_packet_recv;
        loop {
            last_packet_recv = Instant::now();
            // handle packets from the proxy client
            tokio::select! {
                result = reader.next() => {
                    match result {
                        Some(Ok(SocketPacket::ProxyDisconnect(client_id))) => {
                            distributor.lock().await.remove_by_id(client_id);
                        }
                        Some(Ok(SocketPacket::ProxyData(packet))) => {
                            let distributor = distributor.lock().await;
                            let Some(tx) = distributor.get_by_id(packet.client_id) else {
                                continue
                            };
                            if let Err(e) = tx.send(packet.data) {
                                tracing::error!("could not send to minecraft client: {}", e);
                            }
                        }
                        Some(Ok(SocketPacket::ProxyPing(packet))) => {
                            if let Err(e) = tx.send(ClientToProxy::AnswerPingPacket(packet)) {
                                tracing::error!("Could not repsond to ping {e:?}");
                            }
                        }
                        Some(Ok(packet)) => {
                            tracing::info!("Received unexpected proxy packet: {:?}", packet);
                        }
                        None => break, // either the channel was closed or the other side closed the channel or timeout
                        Some(Err(e)) => {
                            tracing::info!("Connection will be closed due to {:?}", e);
                            break;
                        }
                    }
                }
                _ = sleep_until(last_packet_recv.add(Duration::from_secs(TIMEOUT_IN_SEC))) => {
                    tracing::info!("socket timed out");
                    break;
                }
            }
        }
    }
    async fn handle_writer(
        mut writer: SplitSink<Framed<TcpStream, PacketCodec>, SocketPacket>,
        distributor: Arc<Mutex<Distribiutor>>,
        mut rx: UnboundedReceiver<ClientToProxy>,
    ) {
        loop {
            // proxy disconnected and dropped tx
            let Some(mut result) = rx.recv().await else {
                break;
            };
            'inner: loop {
                let socket_packet = match result {
                    ClientToProxy::AddMinecraftClient(id_sender, tx) => {
                        let Ok(client_id) = distributor.lock().await.insert(tx) else {
                            tracing::error!("could not get client id");
                            return;
                        };
                        if let Err(e) = id_sender.send(client_id) {
                            tracing::error!("Could not send back client ID");
                            return;
                        }
                        SocketPacket::ProxyJoin(client_id as ClientID)
                    }
                    ClientToProxy::Packet(id, pkg) => {
                        // if client not found, close connection
                        SocketPacket::from(ProxyDataPacket::new(pkg, id as ClientID))
                    }
                    ClientToProxy::AnswerPingPacket(ping) => SocketPacket::ProxyPong(ping),
                    ClientToProxy::RemoveMinecraftClient(id) => {
                        if let Err(e) = writer.send(SocketPacket::ProxyDisconnect(id)).await {
                            tracing::debug!("Could not write to socket {e:?}");
                            return;
                        }
                        distributor.lock().await.remove_by_id(id);
                        break 'inner;
                    }
                };
                if let Ok(pkg_next) = rx.try_recv() {
                    if let Err(e) = writer.feed(socket_packet).await {
                        tracing::debug!("Could not feed to socket {e:?}");
                        return;
                    };
                    result = pkg_next;
                } else {
                    if let Err(e) = writer.send(socket_packet).await {
                        tracing::debug!("Could not write to socket {e:?}");
                        return;
                    }
                    break 'inner;
                }
            }
        }
    }
    pub async fn register_connection(&mut self) -> Result<(), DistributorError> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.register.add_server(&self.hostname, tx.clone()).await?;
        self.rx = Some(rx);
        self.tx = Some(tx);
        Ok(())
    }
    pub async fn close_connection(&mut self) {
        tracing::info!(
            "removing proxy client {} from state. Connection time: {:?}",
            self.hostname,
            self.connected_time.map(|t| t.elapsed())
        );
        self.register.remove_server(&self.hostname).await;
    }
    pub async fn authenticate(
        &mut self,
        frames: &mut Framed<TcpStream, PacketCodec>,
        packet: &ProxyHelloPacket,
    ) -> Result<(), DistributorError> {
        match &packet.auth {
            ProxyAuthenticator::PublicKey(public_key) => {
                let challenge = public_key.create_challange().map_err(|e| {
                    tracing::error!("Could not create auth challenge: {:?}", e);
                    DistributorError::AuthError
                })?;
                let auth_request = SocketPacket::ProxyAuthRequest(challenge);

                frames.send(auth_request).await?;

                let signature = match frames.next().await {
                    Some(Ok(SocketPacket::ProxyAuthResponse(signature))) => signature,
                    e => {
                        tracing::info!("Client did follow the auth procedure {:?}", e);
                        return Err(DistributorError::WrongPacket);
                    }
                };

                // verify if client posses the private key
                if public_key.verify(&challenge, &signature)
                    && public_key.get_hostname() == packet.hostname
                {
                    tracing::debug!("Client {} authenticated successfully", packet.hostname);
                    return Ok(());
                }
            }
        }
        Err(DistributorError::AuthError)
    }
}
