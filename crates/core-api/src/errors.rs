use crate::crypto::CryptoError;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("incorrect credentials")]
    IncorrectCredentials,

    #[error("network error")]
    Request,

    #[error("network error: {0}")]
    RequestDetailed(#[source] BoxError),

    #[error("decode error: {0}")]
    Decode(#[source] BoxError),

    #[error("protocol error: {0}")]
    Protocol(#[source] BoxError),

    #[error("crypto error: {0}")]
    Crypto(#[from] CryptoError),
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self::RequestDetailed(Box::new(value).into())
    }
}
impl From<base64::DecodeError> for Error {
    fn from(value: base64::DecodeError) -> Self {
        Self::Decode(Box::new(value).into())
    }
}

impl From<opaque_ke::errors::ProtocolError> for Error {
    fn from(value: opaque_ke::errors::ProtocolError) -> Self {
        Self::Protocol(Box::new(value).into())
    }
}
