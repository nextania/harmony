use std::fmt;

use async_std::channel::SendError;
use str0m::error::SdpError;

#[derive(Debug)]
pub enum Error {
    InvalidCall,
    FailedToAuthenticate,
    AlreadyConnected,
    RtcError,
    SocketError,
    SerializeError,
}

impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidCall => write!(f, "Invalid call"),
            Error::FailedToAuthenticate => write!(f, "Failed to authenticate"),
            Error::AlreadyConnected => write!(f, "Already connected"),
            Error::RtcError => write!(f, "RTC error"),
            Error::SocketError => write!(f, "Socket error"),
            Error::SerializeError => write!(f, "Serialize error"),
        }
    }
}

impl From<async_tungstenite::tungstenite::Error> for Error {
    fn from(e: async_tungstenite::tungstenite::Error) -> Self {
        error!("{}", e);
        Error::SocketError
    }
}

impl From<rmp_serde::encode::Error> for Error {
    fn from(e: rmp_serde::encode::Error) -> Self {
        error!("{}", e);
        Error::SerializeError
    }
}

impl From<rmp_serde::decode::Error> for Error {
    fn from(e: rmp_serde::decode::Error) -> Self {
        error!("{}", e);
        Error::SerializeError
    }
}

impl From<async_std::io::Error> for Error {
    fn from(e: async_std::io::Error) -> Self {
        error!("{}", e);
        Error::SocketError
    }
}

impl From<SdpError> for Error {
    fn from(e: SdpError) -> Self {
        error!("{}", e);
        Error::RtcError
    }
}

impl<T> From<SendError<T>> for Error {
    fn from(e: SendError<T>) -> Self {
        error!("{}", e);
        Error::SocketError
    }
}

pub type Result<T> = std::result::Result<T, Error>;
