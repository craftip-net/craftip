use std::ops::Add;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep_until;
use tokio::time::{Duration, Instant};
use tokio_util::codec::Framed;

use shared::addressing::{DistributorError, Register};
use shared::config::{MAXIMUM_CLIENTS, PROTOCOL_VERSION, TIMEOUT_IN_SEC};
use shared::minecraft::MinecraftDataPacket;
use shared::packet_codec::PacketCodec;
use shared::proxy::{
    ProxyAuthenticator, ProxyConnectedResponse, ProxyDataPacket, ProxyHelloPacket,
};
use shared::socket_packet::{ClientID, ClientToProxy, SocketPacket};

#[derive(Debug)]
pub struct Distribiutor {
    clients_id: [Option<UnboundedSender<MinecraftDataPacket>>; MAXIMUM_CLIENTS],
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
    register: Arc<Mutex<Register>>,
    hostname: String,
    rx: Option<UnboundedReceiver<ClientToProxy>>,
    connected_time: Option<Instant>,
}

impl ProxyClient {
    pub fn new(register: Arc<Mutex<Register>>, hostname: &str) -> Self {
        ProxyClient {
            register,
            hostname: hostname.to_string(),
            rx: None,
            connected_time: None,
        }
    }
    /// HANDLE PROXY CLIENT
    pub async fn handle(
        &mut self,
        framed: &mut Framed<TcpStream, PacketCodec>,
    ) -> Result<(), DistributorError> {
        let rx = self.rx.as_mut().unwrap();
        let mut distributor = Distribiutor::default();

        // send connected
        let resp = SocketPacket::from(ProxyConnectedResponse {
            version: PROTOCOL_VERSION,
        });
        framed.send(resp).await?;
        self.connected_time = Some(Instant::now());
        let mut last_packet_recv = Instant::now();
        loop {
            tokio::select! {
                // handle packets from the proxy client
                result = framed.next() => {
                    last_packet_recv = Instant::now();
                    match result {
                        Some(Ok(SocketPacket::ProxyDisconnect(client_id))) => {
                            distributor.remove_by_id(client_id);
                        }
                        Some(Ok(SocketPacket::ProxyData(packet))) => {
                            if let Some(tx) = distributor.get_by_id(packet.client_id) {
                                if let Err(e) = tx.send(packet.data) {
                                    tracing::error!("could not send to minecraft client: {}", e);
                                }
                            }
                        },
                        Some(Ok(SocketPacket::ProxyPing(packet))) => {
                            framed.send(SocketPacket::ProxyPong(packet)).await?
                        }
                        Some(Ok(packet)) => {
                            tracing::info!("Received unexpected proxy packet: {:?}", packet);
                        }
                        None => break, // either the channel was closed or the other side closed the channel or timeout
                        Some(Err(e)) => {
                            tracing::info!("Connection will be closed due to {:?}", e);
                            break
                        }
                    }
                }
                // forward packets from the minecraft clients
                result = rx.recv() => {
                    let mut result = match result {
                        Some(result) => result,
                        None => {
                            tracing::info!("client channel closed {}", self.hostname);
                            break
                        }
                    };
                    'inner: loop {
                        let socket_packet = match result {
                            ClientToProxy::AddMinecraftClient(id_sender, tx) => {
                                let client_id = distributor.insert(tx)?;
                                id_sender.send(client_id).map_err(|_|DistributorError::UnknownError("Send impossible".into()))?;
                                SocketPacket::ProxyJoin(client_id as ClientID)
                            },
                            ClientToProxy::Packet(id, pkg) => {
                                // if client not found, close connection
                                SocketPacket::from(ProxyDataPacket::new(pkg, id as ClientID))
                            },
                            ClientToProxy::RemoveMinecraftClient(id) => {
                                framed.send(SocketPacket::ProxyDisconnect(id)).await?;
                                distributor.remove_by_id(id);
                                break 'inner;
                            }
                        };
                        if let Ok(pkg_next) = rx.try_recv() {
                            framed.feed(socket_packet).await?;
                            result = pkg_next;
                        } else {
                            framed.send(socket_packet).await?;
                            break 'inner;
                        }
                    }
                }
                _ = sleep_until(last_packet_recv.add(Duration::from_secs(TIMEOUT_IN_SEC))) => {
                    tracing::info!("socket timed out");
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn register_connection(&mut self) -> Result<(), DistributorError> {
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let servers = &mut self.register.lock().await.servers;
            if servers.contains_key(&self.hostname) {
                return Err(DistributorError::ServerAlreadyConnected);
            }
            servers.insert(self.hostname.clone(), tx);
        }
        self.rx = Some(rx);
        Ok(())
    }
    pub async fn close_connection(&mut self) {
        tracing::info!(
            "removing proxy client {} from state. Connection time: {:?}",
            self.hostname,
            self.connected_time.map(|t| t.elapsed())
        );
        self.register.lock().await.servers.remove(&self.hostname);
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
