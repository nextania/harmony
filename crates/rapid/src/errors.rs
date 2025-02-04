use std::fmt;

use serde::{Deserialize, Serialize};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "error", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Error {
    // Generic errors
    NotFound,
    Unimplemented,
    InvalidMethod,
    InvalidRequestId,
    InternalError,

    // Authentication errors
    InvalidToken,
    NotAuthenticated,
    SerializeError,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound => write!(f, "Not found"),
            Error::Unimplemented => write!(f, "Unimplemented"),
            Error::InvalidMethod => write!(f, "Invalid method"),
            Error::InvalidRequestId => write!(f, "Invalid request id"),
            Error::InternalError => write!(f, "Internal error"),
            Error::InvalidToken => write!(f, "Invalid token"),
            Error::NotAuthenticated => write!(f, "Not authenticated"),
            Error::SerializeError => write!(f, "Serialize error"),
        }
    }
}

impl From<rmpv::ext::Error> for Error {
    fn from(_: rmpv::ext::Error) -> Self {
        Error::SerializeError
    }
}

impl std::error::Error for Error {}
