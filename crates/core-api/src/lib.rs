//! NOTE: This crate is temporary. It will be merged into the main core repository later.

pub mod crypto;
pub mod errors;

// TODO: implements the AS account API
use std::sync::LazyLock;

use argon2::{Algorithm, Argon2, ParamsBuilder, Version};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as BASE64};
use chacha20poly1305::XChaCha20Poly1305;
use errors::{Error, Result};
use opaque_ke::errors::InternalError;
use opaque_ke::generic_array::{ArrayLength, GenericArray};
use opaque_ke::ksf::Ksf;
use opaque_ke::rand::rngs::OsRng;
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, CredentialResponse, Ristretto255,
    TripleDh,
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::crypto::{derive_key_b, key_b_cipher};

struct DefaultCipherSuite;

impl CipherSuite for DefaultCipherSuite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, sha2::Sha512>;
    type Ksf = ArgonKsf;
}

#[derive(Default)]
struct ArgonKsf {
    argon: Argon2<'static>,
}

impl Ksf for ArgonKsf {
    fn hash<L: ArrayLength<u8>>(
        &self,
        input: GenericArray<u8, L>,
    ) -> std::result::Result<GenericArray<u8, L>, InternalError> {
        let mut output = GenericArray::default();
        self.argon
            .hash_password_into(&input, &[0; argon2::RECOMMENDED_SALT_LEN], &mut output)
            .map_err(|_| InternalError::KsfError)?;
        Ok(output)
    }
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
        encrypted_key: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    Mfa {
        token: String,
        encrypted_key: String,
    },
}

pub enum LoginResult {
    Success(Session),
    RequiresContinuation(LoginMfa),
}

#[derive(Clone)]
pub struct LoginMfa {
    continue_token: String,
    base_url: String,
    password: String,
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

fn get_argon2_ksf() -> ArgonKsf {
    let mut param_builder = ParamsBuilder::default();
    param_builder.t_cost(3);
    param_builder.m_cost(1 << 16);
    param_builder.p_cost(4);

    let params = param_builder
        .build()
        .expect("The provided Argon2 parameters should be valid");
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    ArgonKsf { argon }
}

pub async fn login(base_url: &str, email: &str, password: &str) -> Result<LoginResult> {
    let mut rng = OsRng;

    let login_start = ClientLogin::<DefaultCipherSuite>::start(&mut rng, password.as_bytes())?;

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
        .await?;

    if begin_resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(Error::IncorrectCredentials);
    } else if !begin_resp.status().is_success() {
        return Err(Error::Request);
    }

    let begin_response: LoginResponse = begin_resp.json().await?;

    let (server_message_b64, continue_token) = match begin_response {
        LoginResponse::BeginLogin {
            message,
            continue_token,
        } => (message, continue_token),
        _ => {
            return Err(Error::Request);
        }
    };

    let server_message_bytes = BASE64.decode(&server_message_b64)?;

    let credential_response =
        CredentialResponse::<DefaultCipherSuite>::deserialize(&server_message_bytes)
            .map_err(|_| Error::IncorrectCredentials)?;
    let ksf = get_argon2_ksf();
    let finish_params = ClientLoginFinishParameters::new(None, Default::default(), Some(&ksf));

    let login_finish = login_start
        .state
        .finish(
            &mut rng,
            password.as_bytes(),
            credential_response,
            finish_params,
        )
        .map_err(|_| Error::IncorrectCredentials)?;

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
        .await?;

    if finish_resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(Error::IncorrectCredentials);
    } else if !finish_resp.status().is_success() {
        return Err(Error::Request);
    }

    let finish_response: LoginResponse = finish_resp.json().await?;

    match finish_response {
        LoginResponse::FinishLogin {
            mfa_enabled: false,
            token: Some(token),
            encrypted_key: Some(encrypted_key),
            ..
        } => Ok(LoginResult::Success(Session {
            base_url: base_url.to_string(),
            token,
            key_b: derive_key_b(&encrypted_key, password)?,
        })),
        LoginResponse::FinishLogin {
            mfa_enabled: true,
            continue_token: Some(ct),
            ..
        } => Ok(LoginResult::RequiresContinuation(LoginMfa {
            continue_token: ct,
            base_url: base_url.to_string(),
            password: password.to_string(),
        })),
        _ => Err(Error::IncorrectCredentials),
    }
}

impl LoginMfa {
    pub async fn code(&self, code: &str) -> Result<Session> {
        let payload = Login::Mfa {
            code: code.to_string(),
            continue_token: self.continue_token.clone(),
        };

        let resp = CLIENT
            .post(format!("{}/api/session", self.base_url))
            .json(&payload)
            .send()
            .await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::IncorrectCredentials);
        } else if !resp.status().is_success() {
            return Err(Error::Request);
        }

        let mfa_response: LoginResponse = resp.json().await?;

        match mfa_response {
            LoginResponse::Mfa {
                token,
                encrypted_key,
            } => Ok(Session {
                base_url: self.base_url.clone(),
                token,
                key_b: derive_key_b(&encrypted_key, &self.password)?,
            }),
            _ => Err(Error::IncorrectCredentials),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub description: String,
    pub avatar: Option<AvatarUrl>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarUrl {
    pub id: String,
    pub signature: String,
    pub timestamp: u64,
}

pub struct Session {
    base_url: String,
    token: String,
    key_b: Zeroizing<[u8; 32]>,
}

impl Session {
    // TODO: resume

    pub fn cipher(&self) -> XChaCha20Poly1305 {
        key_b_cipher(&self.key_b)
    }

    pub async fn get_token(&self) -> Result<String> {
        // TODO: finish designing account refresh system?
        Ok(self.token.clone())
    }

    pub async fn get_user(&self, user_id: &str) -> Result<PublicUser> {
        let resp = CLIENT
            .get(format!("{}/api/user/{}", self.base_url, user_id))
            .header("Authorization", &self.token)
            .send()
            .await?;
        let public_user: PublicUser = resp.json().await?;

        Ok(public_user)
    }
    pub async fn get_user_by_username(&self, username: &str) -> Result<PublicUser> {
        let resp = CLIENT
            .get(format!("{}/api/user/username/{}", self.base_url, username))
            .header("Authorization", &self.token)
            .send()
            .await?;
        let public_user: PublicUser = resp.json().await?;

        Ok(public_user)
    }

    pub async fn get_users(&self, user_ids: &[String]) -> Result<Vec<PublicUser>> {
        let resp = CLIENT
            .post(format!("{}/api/user/batch", self.base_url))
            .header("Authorization", &self.token)
            .json(user_ids)
            .send()
            .await?;

        let users: Vec<PublicUser> = resp.json().await?;
        Ok(users)
    }
}
