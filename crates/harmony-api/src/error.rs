//! Error types for the Harmony API client

use thiserror::Error;
pub use harmony_types::errors::Error as ApiError;

/// Result type alias for Harmony API operations
pub type Result<T> = std::result::Result<T, HarmonyError>;

/// Errors that can occur when using the Harmony API client
#[derive(Error, Debug)]
pub enum HarmonyError {
    /// WebSocket connection errors
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] async_tungstenite::tungstenite::Error),

    /// MessagePack serialization/deserialization errors
    #[error("MessagePack serialization error: {0}")]
    MessagePackSerialization(#[from] rmp_serde::encode::Error),

    /// MessagePack deserialization errors
    #[error("MessagePack deserialization error: {0}")]
    MessagePackDeserialization(#[from] rmp_serde::decode::Error),

    /// MessagePack value conversion errors
    #[error("MessagePack value conversion error: {0}")]
    MessagePackValue(#[from] rmpv::decode::Error),

    /// MessagePack ext conversion errors
    #[error("MessagePack ext conversion error: {0}")]
    MessagePackExt(#[from] rmpv::ext::Error),

    /// Authentication errors
    #[error("Authentication failed: {0}")]
    Authentication(String),

    /// API errors returned by the server
    #[error("API error: {0}")]
    Api(#[from] ApiError),

    /// Resource not found
    #[error("Resource not found")]
    NotFound,

    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,

    /// Rate limit exceeded
    #[error("Rate limit exceeded")]
    RateLimit,

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Connection is not established
    #[error("Connection not established")]
    NotConnected,

    /// Connection was lost
    #[error("Connection lost")]
    ConnectionLost,

    /// Reconnection failed after maximum attempts
    #[error("Reconnection failed after {attempts} attempts")]
    ReconnectionFailed { attempts: u32 },

    /// Connection is in the process of reconnecting
    #[error("Connection is reconnecting")]
    Reconnecting,

    /// Internal client error
    #[error("Internal error: {0}")]
    Internal(String),
}
