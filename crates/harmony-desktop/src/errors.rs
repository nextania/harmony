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
    pub fn to_string(&self) -> String {
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
            _ => RenderableError::UnknownError(error.to_string()),
        }
    }
}
