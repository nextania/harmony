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

    #[error("Invalid input: {reason}")]
    InvalidInput { reason: String },

    #[error("Request timed out")]
    Timeout,

    #[error("Not connected")]
    NotConnected,

    #[error("Connection lost")]
    ConnectionLost,

    #[error("Reconnection failed")]
    ReconnectionFailed,

    #[error("Connection is reconnecting")]
    Reconnecting,

    #[error("Internal error: {reason}")]
    Internal { reason: String },

    #[error("Crypto error: {reason}")]
    Crypto { reason: String },
}

impl From<harmony_api::HarmonyError> for HarmonyBindingError {
    fn from(error: harmony_api::HarmonyError) -> Self {
        match error {
            harmony_api::HarmonyError::WebSocket(e) => HarmonyBindingError::WebSocket {
                reason: e.to_string(),
            },
            harmony_api::HarmonyError::Serialization(e) => HarmonyBindingError::Serialization {
                reason: e.to_string(),
            },
            harmony_api::HarmonyError::Http(e) => HarmonyBindingError::Internal {
                reason: e.to_string(),
            },
            harmony_api::HarmonyError::AuthenticationClosed => HarmonyBindingError::Authentication {
                reason: "connection closed before authentication".to_string(),
            },
            harmony_api::HarmonyError::Api(api_error) => HarmonyBindingError::Api {
                reason: format!("{:?}", api_error),
            },
            harmony_api::HarmonyError::InvalidServerUrl(e) => HarmonyBindingError::InvalidInput {
                reason: format!("invalid server URL: {e}"),
            },
            harmony_api::HarmonyError::InvalidGroupKeyLength(len) => {
                HarmonyBindingError::InvalidInput {
                    reason: format!("group key must be exactly 32 bytes, got {len}"),
                }
            }
            harmony_api::HarmonyError::Timeout => HarmonyBindingError::Timeout,
            harmony_api::HarmonyError::NotConnected => HarmonyBindingError::NotConnected,
            harmony_api::HarmonyError::ConnectionLost => HarmonyBindingError::ConnectionLost,
            harmony_api::HarmonyError::ReconnectionFailed { .. } => {
                HarmonyBindingError::ReconnectionFailed
            }
            harmony_api::HarmonyError::Reconnecting => HarmonyBindingError::Reconnecting,
            harmony_api::HarmonyError::KeystoreSyncFailed { attempts } => {
                HarmonyBindingError::Internal {
                    reason: format!("keystore sync failed after {attempts} conflicting writes"),
                }
            }
            error @ (harmony_api::HarmonyError::ContactNotFound
            | harmony_api::HarmonyError::RequesterPublicKeyUnavailable
            | harmony_api::HarmonyError::UnexpectedRelationshipState) => {
                HarmonyBindingError::Internal {
                    reason: error.to_string(),
                }
            }
            harmony_api::HarmonyError::Crypto(e) => HarmonyBindingError::Crypto {
                reason: e.to_string(),
            },
        }
    }
}

pub type HarmonyResult<T> = Result<T, HarmonyBindingError>;
