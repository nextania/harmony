use std::fmt;

use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "error", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Error {
    // Generic errors
    DatabaseError { message: String },
    NotFound,
    Unimplemented,
    InvalidMethod,
    InvalidRequestId,
    InternalError,
    MissingPermission,

    // Authentication errors
    InvalidToken,
    NotAuthenticated,

    // Message errors
    MessageTooLong,
    MessageEmpty,

    // Space errors
    NameTooLong,
    NameEmpty,

    // Invite errors
    InvalidInvite,
    InviteExpired,
    InviteAlreadyUsed,

    // Channel errors
    ChannelFull,
    LastManager,
    NotInChannel,
    InvalidTarget, // For private channels

    // User errors
    Blocked,
    AlreadyEstablished,
    AlreadyRequested,

    // Call errors
    AlreadyExists,
    CallLimitReached,
    NoVoiceNodesAvailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::DatabaseError { message } => write!(f, "Database error: {message}"),
            Error::NotFound => write!(f, "Not found"),
            Error::Unimplemented => write!(f, "Unimplemented"),
            Error::InvalidMethod => write!(f, "Invalid method"),
            Error::InvalidRequestId => write!(f, "Invalid request id"),
            Error::InternalError => write!(f, "Internal error"),
            Error::MissingPermission => write!(f, "Missing permission"),
            Error::InvalidToken => write!(f, "Invalid token"),
            Error::NotAuthenticated => write!(f, "Not authenticated"),
            Error::MessageTooLong => write!(f, "Message too long"),
            Error::MessageEmpty => write!(f, "Message empty"),
            Error::NameTooLong => write!(f, "Name too long"),
            Error::NameEmpty => write!(f, "Name empty"),
            Error::InvalidInvite => write!(f, "Invalid invite"),
            Error::InviteExpired => write!(f, "Invite expired"),
            Error::InviteAlreadyUsed => write!(f, "Invite already used"),
            Error::ChannelFull => write!(f, "Channel full"),
            Error::LastManager => write!(f, "Cannot leave as the last manager"),
            Error::NotInChannel => write!(f, "Not in channel"),
            Error::InvalidTarget => write!(f, "Invalid target"),
            Error::Blocked => write!(f, "Blocked"),
            Error::AlreadyEstablished => write!(f, "Already contacts"),
            Error::AlreadyRequested => write!(f, "Already requested"),
            Error::AlreadyExists => write!(f, "Already exists"),
            Error::CallLimitReached => write!(f, "Call limit reached"),
            Error::NoVoiceNodesAvailable => write!(f, "No voice nodes available"),
        }
    }
}

impl std::error::Error for Error {}

impl From<mongodb::error::Error> for Error {
    fn from(error: mongodb::error::Error) -> Self {
        Error::DatabaseError {
            message: error.to_string(),
        }
    }
}

impl From<mongodb::bson::ser::Error> for Error {
    fn from(error: mongodb::bson::ser::Error) -> Self {
        Error::DatabaseError {
            message: error.to_string(),
        }
    }
}

impl From<redis::RedisError> for Error {
    fn from(error: redis::RedisError) -> Self {
        Error::DatabaseError {
            message: error.to_string(),
        }
    }
}
