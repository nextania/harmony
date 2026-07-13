use std::sync::Arc;
use std::time::Duration;

use pulse_types::MediaHint;

use crate::mls::MlsError;

/// A type-erased error originating from a third-party crate.
pub type SourceError = Arc<dyn std::error::Error + Send + Sync>;

/// Errors returned by the public `PulseClient` API.
#[derive(Clone, Debug, thiserror::Error)]
pub enum PulseError {
    #[error("invalid Pulse server URL: {0}")]
    InvalidUrl(String),

    #[error("transport error: {0}")]
    Transport(#[source] SourceError),

    #[error("server rejected the connection before confirming the join")]
    ConnectRejected,

    #[error("timed out after {0:?} waiting for {1}")]
    Timeout(Duration, &'static str),

    #[error("client is disconnected")]
    Disconnected,

    #[error("already producing a {0:?} track")]
    AlreadyProducing(MediaHint),

    #[error("not producing a {0:?} track")]
    NotProducing(MediaHint),

    #[error("server rejected the request: {0}")]
    Rejected(String),

    #[error("failed to create MoQ broadcast for {0}")]
    BroadcastCreation(String),

    #[error("failed to serialize control message: {0}")]
    ControlSerialization(#[source] SourceError),

    #[error("MLS failure: {0}")]
    Mls(#[from] MlsError),

    #[error("media crypto failure: {0}")]
    Crypto(#[source] MlsError),
}
