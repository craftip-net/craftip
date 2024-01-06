use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;
use tokio_util::codec::Framed;

use shared::addressing::{DistributorError, Register};
use shared::config;
use shared::config::PROTOCOL_VERSION;
use shared::minecraft::MinecraftDataPacket;
use shared::packet_codec::PacketCodec;
use shared::proxy::{
    ProxyAuthenticator, ProxyConnectedResponse, ProxyDataPacket, ProxyHelloPacket,
};
use shared::socket_packet::{ClientToProxy, SocketPacket};

#[derive(Debug, Clone)]
pub struct MinecraftClient {
    tx: UnboundedSender<MinecraftDataPacket>,
    id: u16,
}

#[derive(Debug, Default)]
pub struct Distribiutor {
    clients_addr: HashMap<SocketAddr, MinecraftClient>,
    clients_id: HashMap<u16, SocketAddr>,
}

impl Distribiutor {
    fn insert(
        &mut self,
        addr: SocketAddr,
        tx: UnboundedSender<MinecraftDataPacket>,
    ) -> Result<MinecraftClient, DistributorError> {
        let mut id = None;
        let time = std::time::Instant::now();
        for id_found in 0..=config::MAXIMUM_CLIENTS {
            if !self.clients_id.contains_key(&id_found) {
                id = Some(id_found);
                break;
            }
        }
        tracing::info!("finding id took {:?}", time.elapsed());
        let id = id.ok_or(DistributorError::TooManyClients)?;
        self.clients_id.insert(id, addr);
        let client = MinecraftClient { id, tx };
        self.clients_addr.insert(addr, client.clone());
        Ok(client)
    }
    fn remove_by_addr(&mut self, addr: &SocketAddr) {
        if let Some(client) = self.clients_addr.get(addr) {
            self.clients_id.remove(&client.id);
        }
        self.clients_addr.remove(addr);
    }
    fn remove_by_id(&mut self, id: u16) {
        if let Some(addr) = self.clients_id.get(&id) {
            self.clients_addr.remove(addr);
        }
        self.clients_id.remove(&id);
    }
    fn get_by_addr(&self, addr: &SocketAddr) -> Option<&MinecraftClient> {
        return self.clients_addr.get(addr);
    }
    fn get_by_id(&self, id: u16) -> Option<&MinecraftClient> {
        return self
            .clients_id
            .get(&id)
            .and_then(|addr| self.clients_addr.get(addr));
    }
}

#[derive(Debug)]
pub struct ProxyClient {
    register: Arc<Mutex<Register>>,
    hostname: String,
}

impl ProxyClient {
    pub fn new(register: Arc<Mutex<Register>>, hostname: &str) -> Self {
        ProxyClient {
            register,
            hostname: hostname.to_string(),
        }
    }
    /// HANDLE PROXY CLIENT
    pub async fn handle(
        &mut self,
        framed: &mut Framed<TcpStream, PacketCodec>,
    ) -> Result<(), DistributorError> {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut distributor = Distribiutor::default();

        self.register
            .lock()
            .await
            .servers
            .insert(self.hostname.clone(), tx);

        // send connected
        let resp = SocketPacket::from(ProxyConnectedResponse {
            version: PROTOCOL_VERSION,
        });
        framed.send(resp).await?;
        loop {
            tokio::select! {
                // forward packets from the minecraft clients
                result = rx.recv() => {
                    let result = match result {
                        Some(result) => result,
                        None => {
                            tracing::info!("client channel closed {}", self.hostname);
                            break
                        }
                    };
                    match result {
                        ClientToProxy::Close => {
                            tracing::info!("closing channel for proxy client {}", self.hostname);
                            break
                        },
                        ClientToProxy::AddMinecraftClient(addr, tx) => {
                            let client = distributor.insert(addr, tx)?;
                            framed.send(SocketPacket::ProxyJoin(client.id)).await?;
                        },
                        ClientToProxy::Packet(addr, pkg) => {
                            // if client not found, close connection
                            let client = distributor.get_by_addr(&addr).ok_or_else(||DistributorError::WrongPacket)?;
                            let pkg = SocketPacket::from(ProxyDataPacket::new(pkg, client.id));
                            framed.send(pkg).await?;
                        },
                        ClientToProxy::RemoveMinecraftClient(addr) => {
                            if let Some(client) = distributor.get_by_addr(&addr) {
                                framed.send(SocketPacket::ProxyDisconnect(client.id)).await?;
                            }
                            distributor.remove_by_addr(&addr);
                        }
                    }
                }
                // handle packets from the proxy client
                result = timeout(Duration::from_secs(60), framed.next()) => {
                    // catching timeout error
                    match result {
                        Ok(Some(Ok(packet))) => {
                            match packet {
                                // if mc server disconnects mc client
                                SocketPacket::ProxyDisconnect(client_id) => {
                                    distributor.remove_by_id(client_id);
                                }
                                SocketPacket::ProxyData(packet) => {
                                    if let Some(client) = distributor.get_by_id(packet.client_id) {
                                        let mc_packet = MinecraftDataPacket::from(packet);
                                        if let Err(e) = client.tx.send(mc_packet) {
                                            tracing::error!("could not send to minecraft client: {}", e);
                                        }
                                    }
                                },
                                SocketPacket::ProxyPing(packet) => {
                                    framed.send(SocketPacket::ProxyPong(packet)).await?
                                }
                                packet => {
                                    tracing::info!("Received proxy packet: {:?}", packet);
                                }
                            }
                        }
                        // either the channel was closed or the other side closed the channel or timeout
                        e => {
                            tracing::info!("Connection will be closed due to {:?}", e);
                            break
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub async fn close_connection(&mut self) {
        tracing::info!("removing proxy client {} from state", self.hostname);
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
                    tracing::info!("Client {} authenticated successfully", packet.hostname);
                    return Ok(());
                }
            }
        }
        Err(DistributorError::AuthError)
    }
}
