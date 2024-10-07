use std::collections::HashMap;

use thiserror::Error;
use tokio::sync::mpsc;

use crate::socket_packet::ClientToProxy;

pub type Tx = mpsc::UnboundedSender<ClientToProxy>;
pub type Rx = mpsc::UnboundedReceiver<ClientToProxy>;

/// creates an error string with the file and line number
#[macro_export]
macro_rules! distributor_error {
    ($($arg:tt)*) => ({
        |e| {
            DistributorError::UnknownError(format!("{}:{} {}: {e}", file!(), line!(), format_args!($($arg)*)))
        }
    })
}

#[derive(Debug, Error)]
pub enum DistributorError {
    #[error("ClientNotFound")]
    ClientNotFound,
    #[error("Server Not found")]
    ServerNotFound(String),
    #[error("ServerAlreadyConnected")]
    ServerAlreadyConnected,
    #[error("ServerNotConnected")]
    ServerNotConnected(String),
    #[error("Auth Error")]
    AuthError,
    #[error("Timeout")]
    Timeout,
    #[error("Wrong Packet")]
    WrongPacket,
    #[error("TooManyClients")]
    TooManyClients,
    #[error("UnknownError")]
    UnknownError(String),
    #[error("IO Error")]
    IoError(#[from] std::io::Error),
}

type ServerHostname = String;

#[derive(Debug, Default)]
pub struct Register {
    pub servers: HashMap<ServerHostname, Tx>,
}
