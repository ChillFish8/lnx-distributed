use async_raft::NodeId;
use quinn::{ApplicationClose, ConnectError, ConnectionClose, ConnectionError};

pub type Result<T> = core::result::Result<T, Error>;


#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read TLS file due to error {0}")]
    TlsFileError(std::io::Error),

    #[error("failed to complete TLS operation due to error {0}")]
    TlsError(#[from] rustls::Error),

    #[error("{0}")]
    IoError(#[from] std::io::Error),

    #[error("failed to establish a connection {0}")]
    ClientConnectionError(String),

    #[error("failed to serialize value {0}")]
    SerializationError(#[from] bincode::Error),

    #[error("attempted to interact with a unknown peer node {0}")]
    UnknownNode(NodeId),
}

impl From<ConnectError> for Error {
    fn from(e: ConnectError) -> Self {
        let msg = match e {
            ConnectError::TooManyConnections => "too many connection already exist".to_string(),
            ConnectError::EndpointStopping => "endpoint has been shutdown".to_string(),
            ConnectError::InvalidDnsName(name) => format!("invalid DNS name given {}", name),
            ConnectError::InvalidRemoteAddress(addr) => format!("invalid address given {}", addr),
            ConnectError::NoDefaultClientConfig => unreachable!(),
            ConnectError::UnsupportedVersion => "cryptographic layer does not support the specified QUIC version".to_string(),
        };

        Self::ClientConnectionError(msg)
    }
}

impl From<ConnectionError> for Error {
    fn from(e: ConnectionError) -> Self {
        let msg = match e {
            ConnectionError::VersionMismatch => "peer does not support any compatible versions".to_string(),
            ConnectionError::TransportError(e) => format!("transport error {}", e),
            ConnectionError::ConnectionClosed(e) => match e {
                ConnectionClose { error_code, reason, .. } =>
                    format!("client connection closed. code={}, reason={:?}", error_code, reason)
            },
            ConnectionError::ApplicationClosed(e) => match e {
                ApplicationClose { error_code, reason } =>
                    format!("peer aborted connection closed. code={}, reason={:?}", error_code, reason)
            },
            ConnectionError::Reset => "connection was unexpectedly reset".to_string(),
            ConnectionError::TimedOut => "connection timed out".to_string(),
            ConnectionError::LocallyClosed => "local connection aborted the connection".to_string(),
        };

        Self::ClientConnectionError(msg)
    }
}

