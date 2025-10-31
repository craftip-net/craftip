use std::net::SocketAddr;
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
pub struct Distributor {
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

impl Default for Distributor {
    fn default() -> Self {
        const CHANNEL_NONE: Option<UnboundedSender<MinecraftDataPacket>> = None;
        Self {
            clients_id: [CHANNEL_NONE; MAXIMUM_CLIENTS],
        }
    }
}
impl Distributor {
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

    /// Returns true if existed
    fn remove_by_id(&mut self, id: ClientID) -> bool {
        if let Some(client) = self.clients_id.get_mut(id) {
            client.take();
            return true;
        }
        false
    }

    fn get_by_id(&self, id: ClientID) -> Option<&UnboundedSender<MinecraftDataPacket>> {
        let sender = self.clients_id.get(id);
        return sender.and_then(|inner| inner.as_ref());
    }
}

pub struct Authenticated {
    framed: Option<Framed<TcpStream, PacketCodec>>,
}
pub struct Disconnected;

pub struct NotAuthenticated {
    hello: ProxyHelloPacket,
    framed: Framed<TcpStream, PacketCodec>,
}

#[derive(Debug)]
pub struct ProxyClient<State = NotAuthenticated> {
    register: Register,
    hostname: String,
    state: State,
}

impl ProxyClient {
    pub async fn new(
        register: Register,
        mut framed: Framed<TcpStream, PacketCodec>,
    ) -> Result<ProxyClient<NotAuthenticated>, DistributorError> {
        let hello_packet = loop {
            match framed.next().await {
                Some(Ok(SocketPacket::ProxyHello(packet))) => {
                    break packet;
                }
                Some(Ok(SocketPacket::ProxyPing(ping))) => {
                    framed.send(SocketPacket::ProxyPong(ping)).await?;
                    continue;
                }
                e => {
                    tracing::error!("Wrong first packet! {:?}", e);
                    return Err(DistributorError::WrongPacket);
                }
            };
        };
        Ok(ProxyClient {
            register,
            hostname: hello_packet.hostname.clone(),
            state: NotAuthenticated {
                hello: hello_packet,
                framed,
            },
        })
    }
}
impl ProxyClient<Authenticated> {
    #[inline]
    fn into_disconnected_unsafe_proxy_not_registered_anymore(self) -> ProxyClient<Disconnected> {
        ProxyClient {
            register: self.register,
            hostname: self.hostname,
            state: Disconnected,
        }
    }
    async fn into_disconnected(self) -> ProxyClient<Disconnected> {
        self.register.remove_server(&self.hostname).await;
        self.into_disconnected_unsafe_proxy_not_registered_anymore()
    }

    /// HANDLE PROXY CLIENT
    pub async fn handle(mut self, ip: &SocketAddr) -> ProxyClient<Disconnected> {
        // can't be executed twice, since `handle` consumes proxy
        let mut framed = self.state.framed.take().unwrap();
        let (tx, rx) = mpsc::unbounded_channel();
        if let Err(_e) = self.register.add_server(&self.hostname, tx.clone()).await {
            tracing::info!("Can't connect {}. Already connected?", self.hostname);
            let _res = framed
                .send(SocketPacket::ProxyError("Already connected?".to_string()))
                .await;
            // one exception, because register.add failed
            return self.into_disconnected_unsafe_proxy_not_registered_anymore();
        }

        // send connected
        let resp = SocketPacket::from(ProxyConnectedResponse {
            version: PROTOCOL_VERSION,
        });
        if let Err(e) = framed.send(resp).await {
            tracing::debug!("Sending hello response failed. {e:?}");
            return self.into_disconnected().await;
        }
        tracing::info!("Proxy client {} connected from {:?}", self.hostname, ip);

        let connected_time = Some(Instant::now());

        let (writer, reader) = framed.split::<SocketPacket>();

        let distributor = Arc::new(Mutex::new(Distributor::default()));

        let reader = task::spawn(ProxyClient::handle_reader(reader, distributor.clone(), tx));
        let writer = task::spawn(ProxyClient::handle_writer(writer, distributor.clone(), rx));
        let res = select(reader, writer).await;
        // terminate the other task
        res.factor_second().1.abort();

        tracing::info!(
            "removing proxy client {} from state. Connection time: {:?}",
            self.hostname,
            connected_time.map(|t| t.elapsed())
        );
        self.into_disconnected().await
    }

    async fn handle_reader(
        mut reader: SplitStream<Framed<TcpStream, PacketCodec>>,
        distributor: Arc<Mutex<Distributor>>,
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
                                tracing::error!("Could not respond to ping {e:?}");
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
        distributor: Arc<Mutex<Distributor>>,
        mut rx: UnboundedReceiver<ClientToProxy>,
    ) {
        loop {
            let Some(mut packet) = rx.recv().await else {
                tracing::error!("ClientToProxy channel was dropped");
                break;
            };

            // Makes sure that the rx channel gets drained so that write doesn't block this process
            'feed: loop {
                let resp = match packet {
                    ClientToProxy::AddMinecraftClient(id_sender, tx) => {
                        let Ok(client_id) = distributor.lock().await.insert(tx) else {
                            tracing::error!("could not get client id (Too many clients?)");
                            return;
                        };
                        if let Err(e) = id_sender.send(client_id) {
                            tracing::error!("Could not send back client ID {e:?}");
                            return;
                        }
                        Some(SocketPacket::ProxyJoin(client_id as ClientID))
                    }
                    ClientToProxy::Packet(id, pkg) => {
                        // if client not found, close connection
                        Some(SocketPacket::from(ProxyDataPacket::new(
                            pkg,
                            id as ClientID,
                        )))
                    }
                    ClientToProxy::AnswerPingPacket(ping) => Some(SocketPacket::ProxyPong(ping)),
                    ClientToProxy::RemoveMinecraftClient(id) => {
                        if distributor.lock().await.remove_by_id(id) {
                            Some(SocketPacket::ProxyDisconnect(id))
                        } else {
                            // If not present anymore, it has already been removed. We don't want infinite acks
                            None
                        }
                    }
                };

                if let Some(packet) = resp {
                    if let Err(e) = writer.feed(packet).await {
                        tracing::info!("Could not feed to socket {e:?}");
                        return;
                    };
                }

                match rx.try_recv() {
                    Ok(res) => packet = res,
                    Err(_) => break 'feed,
                }
            }

            if let Err(e) = &mut writer.flush().await {
                tracing::info!("Could not flush socket: {}", e);
                return;
            }
        }
    }
}

impl ProxyClient<NotAuthenticated> {
    pub async fn authenticate(mut self) -> Result<ProxyClient<Authenticated>, DistributorError> {
        match &self.state.hello.auth {
            ProxyAuthenticator::PublicKey(public_key) => {
                let challenge = public_key.create_challange().map_err(|e| {
                    tracing::error!("Could not create auth challenge: {:?}", e);
                    DistributorError::AuthError
                })?;
                let auth_request = SocketPacket::ProxyAuthRequest(challenge);

                self.state.framed.send(auth_request).await?;

                let signature = match self.state.framed.next().await {
                    Some(Ok(SocketPacket::ProxyAuthResponse(signature))) => signature,
                    e => {
                        let addr = self.state.framed.get_ref().peer_addr();
                        tracing::error!("Client (addr: {:?}) trying to authenticate for {} did follow the auth procedure: {:?}", addr, self.hostname, e);
                        let e = SocketPacket::ProxyError(
                            "Client did follow the auth procedure".to_string(),
                        );
                        let _res = self.state.framed.send(e).await;
                        return Err(DistributorError::WrongPacket);
                    }
                };

                // verify if client posses the private key
                if !public_key.verify(&challenge, &signature)
                    || public_key.get_hostname() != self.hostname
                {
                    tracing::error!("Auth key did not match {}", self.hostname);
                    let e = SocketPacket::ProxyError("Could not authenticate".to_string());
                    let _res = self.state.framed.send(e).await;
                    return Err(DistributorError::AuthError);
                }

                tracing::debug!("Client {} authenticated successfully", self.hostname);
                Ok(ProxyClient {
                    register: self.register,
                    hostname: self.hostname,
                    state: Authenticated {
                        framed: Some(self.state.framed),
                    },
                })
            }
        }
    }
}
