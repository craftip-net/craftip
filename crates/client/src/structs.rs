use serde::{Deserialize, Serialize};
use shared::crypto::ServerPrivateKey;
use shared::minecraft::MinecraftDataPacket;
use shared::packet_codec::PacketCodecError;
use shared::socket_packet::{ClientID, PingPacket};
use std::io;
use thiserror::Error;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Debug)]
pub enum Stats {
    Connected,
    ClientsConnected(usize),
    Ping(u16),
}

#[derive(Debug)]
pub enum Control {
    Disconnect,
}

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("Io Error: {0}")]
    Io(#[from] io::Error),
    #[error("protocol error: {0}")]
    ProtocolError(#[from] PacketCodecError),
    #[error("Proxy closed the connection")]
    ProxyClosedConnection,
    #[error("User closed the connection")]
    UserClosedConnection,
    #[error("Timeout")]
    Timeout,
    #[error("Proxy error: {0}")]
    ProxyError(String),
    #[error("Minecraft server error. Is the server running?")]
    MinecraftServerNotFound,
    #[error("Unexpected packet: {0}")]
    UnexpectedPacket(String),
    #[error("Other error: {0}")]
    Other(#[from] anyhow::Error),
}

pub enum ClientToProxy {
    Packet(ClientID, MinecraftDataPacket),
    RemoveMinecraftClient(ClientID),
    Ping(PingPacket),
    Death(String),
}
pub type ClientToProxyRx = UnboundedReceiver<ClientToProxy>;
pub type ClientToProxyTx = UnboundedSender<ClientToProxy>;
pub type ProxyToClient = MinecraftDataPacket;
pub type ProxyToClientRx = UnboundedReceiver<ProxyToClient>;
pub type ProxyToClientTx = UnboundedSender<ProxyToClient>;
pub type ControlTx = UnboundedSender<Control>;
pub type ControlRx = UnboundedReceiver<Control>;

pub type StatsTx = UnboundedSender<Stats>;
pub type StatsRx = UnboundedReceiver<Stats>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Server {
    pub server: String,
    pub local: String,
    pub auth: ServerAuthentication,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerAuthentication {
    Key(ServerPrivateKey),
}
pub const STANDARD_LOCAL_PORT: &str = "25565";
impl Server {
    pub fn new_from_key(key: ServerPrivateKey) -> Self {
        let id = key.get_public_key().get_host();
        Self {
            server: format!("{}{}", id, shared::config::KEY_SERVER_SUFFIX),
            local: STANDARD_LOCAL_PORT.to_string(),
            auth: ServerAuthentication::Key(key),
        }
    }
}
