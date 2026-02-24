#[derive(Debug, Clone, uniffi::Error, thiserror::Error)]
#[uniffi(flat_error)]
pub enum HarmonyBindingError {
    #[error("WebSocket error: {reason}")]
    WebSocket { reason: String },

    #[error("Serialization error: {reason}")]
    Serialization { reason: String },

    #[error("Authentication failed: {reason}")]
    Authentication { reason: String },

    #[error("API error: {reason}")]
    Api { reason: String },

    #[error("Resource not found")]
    NotFound,

    #[error("Permission denied")]
    PermissionDenied,

    #[error("Invalid input: {reason}")]
    InvalidInput { reason: String },

    #[error("Not connected")]
    NotConnected,

    #[error("Connection lost")]
    ConnectionLost,

    #[error("Reconnection failed")]
    ReconnectionFailed,

    #[error("Connection is reconnecting")]
    Reconnecting,

    #[error("Rate limit exceeded")]
    RateLimit,

    #[error("Internal error: {reason}")]
    Internal { reason: String },
}

impl From<harmony_api::HarmonyError> for HarmonyBindingError {
    fn from(error: harmony_api::HarmonyError) -> Self {
        match error {
            harmony_api::HarmonyError::WebSocket(e) => HarmonyBindingError::WebSocket {
                reason: e.to_string(),
            },
            harmony_api::HarmonyError::MessagePackSerialization(e) => {
                HarmonyBindingError::Serialization {
                    reason: e.to_string(),
                }
            }
            harmony_api::HarmonyError::MessagePackDeserialization(e) => {
                HarmonyBindingError::Serialization {
                    reason: e.to_string(),
                }
            }
            harmony_api::HarmonyError::MessagePackValue(e) => HarmonyBindingError::Serialization {
                reason: e.to_string(),
            },
            harmony_api::HarmonyError::MessagePackExt(e) => HarmonyBindingError::Serialization {
                reason: e.to_string(),
            },
            harmony_api::HarmonyError::Authentication(reason) => {
                HarmonyBindingError::Authentication { reason }
            }
            harmony_api::HarmonyError::Api(api_error) => HarmonyBindingError::Api {
                reason: format!("{:?}", api_error),
            },
            harmony_api::HarmonyError::NotFound => HarmonyBindingError::NotFound,
            harmony_api::HarmonyError::PermissionDenied => HarmonyBindingError::PermissionDenied,
            harmony_api::HarmonyError::InvalidInput(reason) => {
                HarmonyBindingError::InvalidInput { reason }
            }
            harmony_api::HarmonyError::NotConnected => HarmonyBindingError::NotConnected,
            harmony_api::HarmonyError::ConnectionLost => HarmonyBindingError::ConnectionLost,
            harmony_api::HarmonyError::ReconnectionFailed { attempts } => {
                HarmonyBindingError::ReconnectionFailed
            }
            harmony_api::HarmonyError::Reconnecting => HarmonyBindingError::Reconnecting,
            harmony_api::HarmonyError::RateLimit => HarmonyBindingError::RateLimit,
            harmony_api::HarmonyError::Internal(reason) => HarmonyBindingError::Internal { reason },
        }
    }
}

pub type HarmonyResult<T> = Result<T, HarmonyBindingError>;
