pub use harmony_types::errors::Error as ApiError;
use thiserror::Error;

pub use crate::crypto::CryptoError;
pub use core_api::errors::Error as CoreError;

/// A type-erased error originating from a third-party crate.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Result type alias for Harmony API operations
pub type Result<T> = std::result::Result<T, HarmonyError>;

/// Errors that can occur when using the Harmony API client
#[derive(Error, Debug)]
pub enum HarmonyError {
    #[error("WebSocket error: {0}")]
    WebSocket(#[source] BoxError),

    #[error("CBOR serialization error: {0}")]
    Serialization(#[source] BoxError),

    #[error("HTTP request error: {0}")]
    Http(#[source] BoxError),

    #[error("invalid server URL: {0}")]
    InvalidServerUrl(#[source] BoxError),

    #[error("connection closed before authentication")]
    AuthenticationClosed,

    #[error("API error: {0}")]
    Api(#[from] ApiError),

    #[error("request timed out")]
    Timeout,

    #[error("connection not established")]
    NotConnected,

    #[error("connection lost")]
    ConnectionLost,

    #[error("reconnection failed after {attempts} attempts")]
    ReconnectionFailed { attempts: u32 },

    #[error("connection is reconnecting")]
    Reconnecting,

    #[error("keystore sync failed after {attempts} conflicting writes")]
    KeystoreSyncFailed { attempts: u32 },

    #[error("contact not found")]
    ContactNotFound,

    #[error("cannot accept: requester's public key not available")]
    RequesterPublicKeyUnavailable,

    #[error("expected an Established relationship state after finalizing contact")]
    UnexpectedRelationshipState,

    #[error("group key must be exactly 32 bytes, got {0}")]
    InvalidGroupKeyLength(usize),

    #[error("Crypto error: {0}")]
    Crypto(#[from] CryptoError),

    #[error("Core error: {0}")]
    Core(#[from] CoreError),
}
