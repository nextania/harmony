use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "error", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Error {
    // Generic errors
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

    // Authentication errors
    #[error("Invalid token")]
    InvalidToken,
    #[error("Not authenticated")]
    NotAuthenticated,
    #[error("Serialize error")]
    SerializeError(#[from] #[serde(skip)] rmpv::ext::Error),

    // Rate limiting
    #[error("Rate limited")]
    RateLimited,
}
