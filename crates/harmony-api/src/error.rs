//! Error types for the Harmony API client

use thiserror::Error;

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
    Api(ApiError),

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

/// API errors that match the server's error format
#[derive(Error, Debug, Clone, serde::Deserialize)]
#[serde(tag = "error", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ApiError {
    /// Generic errors
    #[error("Database error: {message}")]
    DatabaseError { message: String },

    #[error("Not found")]
    NotFound,

    #[error("Unimplemented")]
    Unimplemented,

    #[error("Invalid method")]
    InvalidMethod,

    #[error("Invalid request ID")]
    InvalidRequestId,

    #[error("Internal error")]
    InternalError,

    #[error("Missing permission")]
    MissingPermission,

    /// Authentication errors
    #[error("Invalid token")]
    InvalidToken,

    #[error("Not authenticated")]
    NotAuthenticated,

    /// Message errors
    #[error("Message too long")]
    MessageTooLong,

    #[error("Message empty")]
    MessageEmpty,

    /// Space errors
    #[error("Name too long")]
    NameTooLong,

    #[error("Name empty")]
    NameEmpty,

    /// Invite errors
    #[error("Invalid invite")]
    InvalidInvite,

    #[error("Invite expired")]
    InviteExpired,

    #[error("Invite already used")]
    InviteAlreadyUsed,

    /// Channel errors
    #[error("Channel full")]
    ChannelFull,

    /// User errors
    #[error("Blocked")]
    Blocked,

    #[error("Already established")]
    AlreadyEstablished,

    #[error("Already requested")]
    AlreadyRequested,

    #[error("Not friends")]
    NotFriends,

    /// Call errors
    #[error("Already exists")]
    AlreadyExists,

    #[error("Call limit reached")]
    CallLimitReached,

    #[error("No voice nodes available")]
    NoVoiceNodesAvailable,
}

#[derive(Debug, serde::Deserialize)]
pub struct ApiErrorResponse {
    pub error: ApiError,
}

impl From<ApiError> for HarmonyError {
    fn from(err: ApiError) -> Self {
        match err {
            ApiError::NotFound => HarmonyError::NotFound,
            ApiError::MissingPermission => HarmonyError::PermissionDenied,
            ApiError::InvalidToken | ApiError::NotAuthenticated => {
                HarmonyError::Authentication(err.to_string())
            }
            other => HarmonyError::Api(other),
        }
    }
}
