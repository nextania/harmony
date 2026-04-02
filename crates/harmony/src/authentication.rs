use std::{
    any::Any,
    sync::{Arc, LazyLock},
};

use rapid::socket::RpcState;
use serde::Deserialize;
use serde_json::json;

use crate::{
    errors::{Error, Result},
    services::{
        database::users::User,
        environment::{AS_TOKEN, AS_URI},
    },
};

// Important: This only accepts a token and will not sign a token.
// The token is to be obtained from a separate login server
// (e.g. AS)
pub async fn authenticate(
    token: String,
) -> rapid::errors::Result<(String, Box<dyn Any + Send + Sync>)> {
    let as_user = validate_token(&token).await?;
    let user = User::get(&as_user.id).await;
    let user = if let Err(Error::NotFound) = user {
        User::create(as_user.id)
            .await
            .map_err(|_| rapid::errors::Error::InternalError)?
    } else {
        user.map_err(|_| rapid::errors::Error::InternalError)?
    };
    Ok((user.id.clone(), Box::new(user)))
}
static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

#[derive(Clone, Debug, Deserialize)]
pub struct AsUser {
    pub id: String,
}

pub async fn validate_token(token: &str) -> rapid::errors::Result<AsUser> {
    let resp = CLIENT
        .get(format!("{}/api/introspect", AS_URI.as_str()))
        .header("Authorization", AS_TOKEN.as_str())
        .body(json!({ "token": token }).to_string())
        .send()
        .await
        .map_err(|_| rapid::errors::Error::InternalError)?;
    if !resp.status().is_success() {
        return Err(rapid::errors::Error::InvalidToken);
    }
    let as_user = resp
        .json::<AsUser>()
        .await
        .map_err(|_| rapid::errors::Error::InternalError)?;
    Ok(as_user)
}

pub fn check_authenticated(state: &RpcState) -> Result<Arc<User>> {
    state
        .client()
        .get_user::<User>()
        .cloned()
        .ok_or(Error::NotAuthenticated)
        .map(|user| user.into())
}
