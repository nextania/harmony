use core_api::errors::Error as CoreError;
use harmony_api::HarmonyError;

/// A `Result` type with localizable error messages.
pub type RenderableResult<T> = Result<T, RenderableError>;

#[derive(Debug, Clone)]
pub enum RenderableError {
    IncorrectCredentials,
    NetworkError,
    CryptoError(String),
    UnknownError(String),
}

// TODO: localize error messages
impl RenderableError {
    pub fn friendly(&self) -> String {
        match self {
            RenderableError::IncorrectCredentials => "Invalid email or password".into(),
            RenderableError::NetworkError => "Network error, please try again".into(),
            RenderableError::CryptoError(msg) => format!("Encryption error: {}", msg),
            RenderableError::UnknownError(msg) => format!("An unknown error occurred: {}", msg),
        }
    }
}

impl From<HarmonyError> for RenderableError {
    fn from(error: HarmonyError) -> Self {
        match error {
            HarmonyError::Crypto(e) => RenderableError::CryptoError(e.to_string()),
            HarmonyError::NotConnected
            | HarmonyError::ConnectionLost
            | HarmonyError::Timeout
            | HarmonyError::Reconnecting
            | HarmonyError::ReconnectionFailed { .. } => RenderableError::NetworkError,
            _ => RenderableError::UnknownError(error.to_string()),
        }
    }
}

impl From<CoreError> for RenderableError {
    fn from(error: CoreError) -> Self {
        match error {
            CoreError::IncorrectCredentials => RenderableError::IncorrectCredentials,
            CoreError::Crypto(msg) => RenderableError::CryptoError(msg.to_string()),
            CoreError::Request => RenderableError::NetworkError,
            CoreError::RequestDetailed(_) => RenderableError::NetworkError,
            _ => RenderableError::UnknownError(error.to_string()),
        }
    }
}
