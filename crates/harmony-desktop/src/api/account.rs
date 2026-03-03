// implements the AS account API

use std::sync::LazyLock;

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use opaque_ke::rand::rngs::OsRng;
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, CredentialResponse, Ristretto255,
    TripleDh,
};
use serde::{Deserialize, Serialize};

use crate::errors::RenderableError;

struct DefaultCipherSuite;

impl CipherSuite for DefaultCipherSuite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, sha2::Sha512>;
    type Ksf = opaque_ke::ksf::Identity;
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "stage")]
pub enum Login {
    BeginLogin {
        email: String,
        message: String,
        escalate: bool,
        token: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    FinishLogin {
        message: String,
        continue_token: String,
        persist: Option<bool>,
        friendly_name: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Mfa {
        code: String,
        continue_token: String,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum LoginResponse {
    #[serde(rename_all = "camelCase")]
    BeginLogin {
        continue_token: String,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    FinishLogin {
        mfa_enabled: bool,
        continue_token: Option<String>,
        token: Option<String>,
    },
    Mfa {
        token: String,
    },
}

pub enum LoginResult {
    Success(String),
    RequiresContinuation(LoginMfa),
}

#[derive(Clone)]
pub struct LoginMfa {
    continue_token: String,
    base_url: String,
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

pub async fn login(
    base_url: &str,
    email: &str,
    password: &str,
) -> Result<LoginResult, RenderableError> {
    let mut rng = OsRng;

    let login_start = ClientLogin::<DefaultCipherSuite>::start(&mut rng, password.as_bytes())
        .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

    let credential_request_b64 = BASE64.encode(login_start.message.serialize());

    let begin_payload = Login::BeginLogin {
        email: email.to_string(),
        message: credential_request_b64,
        escalate: false,
        token: None,
    };

    let begin_resp = CLIENT
        .post(format!("{}/api/session", base_url))
        .json(&begin_payload)
        .send()
        .await
        .map_err(|_| RenderableError::NetworkError)?;

    if begin_resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RenderableError::IncorrectCredentials);
    } else if !begin_resp.status().is_success() {
        return Err(RenderableError::NetworkError);
    }

    let begin_response: LoginResponse = begin_resp
        .json()
        .await
        .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

    let (server_message_b64, continue_token) = match begin_response {
        LoginResponse::BeginLogin {
            message,
            continue_token,
        } => (message, continue_token),
        _ => {
            return Err(RenderableError::UnknownError(
                "Unexpected response stage".to_string(),
            ));
        }
    };

    let server_message_bytes = BASE64
        .decode(&server_message_b64)
        .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

    let credential_response =
        CredentialResponse::<DefaultCipherSuite>::deserialize(&server_message_bytes)
            .map_err(|_| RenderableError::IncorrectCredentials)?;

    let login_finish = login_start
        .state
        .finish(
            &mut rng,
            password.as_bytes(),
            credential_response,
            ClientLoginFinishParameters::default(),
        )
        .map_err(|_| RenderableError::IncorrectCredentials)?;

    let finalization_b64 = BASE64.encode(login_finish.message.serialize());

    let finish_payload = Login::FinishLogin {
        message: finalization_b64,
        continue_token,
        persist: None,
        friendly_name: None,
    };

    let finish_resp = CLIENT
        .post(format!("{}/api/session", base_url))
        .json(&finish_payload)
        .send()
        .await
        .map_err(|_| RenderableError::NetworkError)?;

    if finish_resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(RenderableError::IncorrectCredentials);
    } else if !finish_resp.status().is_success() {
        return Err(RenderableError::NetworkError);
    }

    let finish_response: LoginResponse = finish_resp
        .json()
        .await
        .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

    match finish_response {
        LoginResponse::FinishLogin {
            mfa_enabled: false,
            token: Some(token),
            ..
        } => Ok(LoginResult::Success(token)),
        LoginResponse::FinishLogin {
            mfa_enabled: true,
            continue_token: Some(ct),
            ..
        } => Ok(LoginResult::RequiresContinuation(LoginMfa {
            continue_token: ct,
            base_url: base_url.to_string(),
        })),
        _ => Err(RenderableError::IncorrectCredentials),
    }
}

impl LoginMfa {
    pub async fn code(&self, code: &str) -> Result<String, RenderableError> {
        let payload = Login::Mfa {
            code: code.to_string(),
            continue_token: self.continue_token.clone(),
        };

        let resp = CLIENT
            .post(format!("{}/api/session", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|_| RenderableError::NetworkError)?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RenderableError::IncorrectCredentials);
        } else if !resp.status().is_success() {
            return Err(RenderableError::NetworkError);
        }

        let mfa_response: LoginResponse = resp
            .json()
            .await
            .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

        match mfa_response {
            LoginResponse::Mfa { token } => Ok(token),
            _ => Err(RenderableError::IncorrectCredentials),
        }
    }
}
