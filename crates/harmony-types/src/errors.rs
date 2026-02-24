use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Error, Deserialize, Serialize)]
#[serde(tag = "error", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Error {
    // Generic errors
    #[error("Database error: {message}")]
    DatabaseError { message: String },
    #[error("Not found")]
    NotFound,
    #[error("Unimplemented")]
    Unimplemented,
    #[error("Invalid method")]
    InvalidMethod,
    #[error("Invalid request id")]
    InvalidRequestId,
    #[error("Internal error")]
    InternalError,
    #[error("Missing permission")]
    MissingPermission,

    // Authentication errors
    #[error("Invalid token")]
    InvalidToken,
    #[error("Not authenticated")]
    NotAuthenticated,

    // Message errors
    #[error("Message too long")]
    MessageTooLong,
    #[error("Message empty")]
    MessageEmpty,

    // Space errors
    #[error("Name too long")]
    NameTooLong,
    #[error("Name empty")]
    NameEmpty,

    // Invite errors
    #[error("Invalid invite")]
    InvalidInvite,
    #[error("Invite expired")]
    InviteExpired,
    #[error("Invite already used")]
    InviteAlreadyUsed,

    // Channel errors
    #[error("Channel full")]
    ChannelFull,
    #[error("Cannot leave as the last manager")]
    LastManager,
    #[error("Not in channel")]
    NotInChannel,
    #[error("Invalid target")]
    InvalidTarget, // For private channels

    // User errors
    #[error("Blocked")]
    Blocked,
    #[error("Already contacts")]
    AlreadyEstablished,
    #[error("Already requested")]
    AlreadyRequested,

    // Call errors
    #[error("Already exists")]
    AlreadyExists,
    #[error("Call limit reached")]
    CallLimitReached,
    #[error("No voice nodes available")]
    NoVoiceNodesAvailable,
}

#[cfg(feature = "server")]
impl From<mongodb::error::Error> for Error {
    fn from(error: mongodb::error::Error) -> Self {
        Error::DatabaseError {
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "server")]
impl From<mongodb::bson::ser::Error> for Error {
    fn from(error: mongodb::bson::ser::Error) -> Self {
        Error::DatabaseError {
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "server")]
impl From<redis::RedisError> for Error {
    fn from(error: redis::RedisError) -> Self {
        Error::DatabaseError {
            message: error.to_string(),
        }
    }
}

#[cfg(feature = "server")]
impl rapid::socket::RpcResponder for Error {
    fn into_value(self) -> rmpv::Value {
        rmpv::ext::to_value(self).unwrap()
    }
}
