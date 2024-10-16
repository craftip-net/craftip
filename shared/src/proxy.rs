use crate::crypto::{ChallengeDataType, ServerPublicKey, SignatureDataType};
use crate::minecraft::MinecraftDataPacket;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

/// ProxyHelloPacket is the first packet sent by the client to the proxy.
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct ProxyHelloPacket {
    pub version: u16,
    pub hostname: String,
    pub auth: ProxyAuthenticator,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum ProxyAuthenticator {
    PublicKey(ServerPublicKey),
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum ProxyHandshakeResponse {
    ConnectionSuccessful(),
    Err(String),
}
#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum ProxyAuthRequestPacket {
    #[serde(with = "BigArray")]
    PublicKey(ChallengeDataType),
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub enum ProxyAuthResponePacket {
    #[serde(with = "BigArray")]
    PublicKey(SignatureDataType),
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct ProxyConnectedResponse {
    pub version: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct ProxyClientJoinPacket {
    pub client_id: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct ProxyClientDisconnectPacket {
    pub client_id: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, PartialEq)]
pub struct ProxyDataPacket {
    pub client_id: u16,
    pub data: MinecraftDataPacket,
}

impl ProxyDataPacket {
    pub fn new(data: MinecraftDataPacket, client_id: u16) -> Self {
        Self { client_id, data }
    }
}

/// ProxyClientJoinPacket constructor
impl ProxyClientJoinPacket {
    pub fn new(client_id: u16) -> Self {
        ProxyClientJoinPacket { client_id }
    }
}

/// ProxyClientDisconnectPacket constructor
impl ProxyClientDisconnectPacket {
    pub fn new(client_id: u16) -> Self {
        ProxyClientDisconnectPacket { client_id }
    }
}
