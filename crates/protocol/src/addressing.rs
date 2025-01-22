use thiserror::Error;
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
    #[error("Server \"{0}\" Not found")]
    ServerNotFound(String),
    #[error("ServerAlreadyConnected")]
    ServerAlreadyConnected,
    #[error("Server \"{0}\" not connected")]
    ServerNotConnected(String),
    #[error("Auth Error")]
    AuthError,
    #[error("Timeout")]
    Timeout,
    #[error("Wrong Packet")]
    WrongPacket,
    #[error("TooManyClients")]
    TooManyClients,
    #[error("Unknown Error: {0}")]
    UnknownError(String),
    #[error("IO Error")]
    IoError(#[from] std::io::Error),
}
