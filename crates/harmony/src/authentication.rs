use std::{any::Any, sync::Arc};

use dashmap::DashMap;
use once_cell::sync::Lazy;
use rapid::socket::{RpcClient, RpcResponder};
use rmpv::{Value, ext::to_value};
use serde::Deserialize;
use serde_json::json;

use crate::{
    errors::{Error, Result},
    services::{database::users::User, environment::{AS_TOKEN, AS_URI}},
};

// Important: This only accepts a token and will not sign a token.
// The token is to be obtained from a separate login server
// (e.g. AS)
pub async fn authenticate(token: String) -> rapid::errors::Result<Box<dyn Any + Send + Sync>> {
    // println!("Public key: {:?}", self.public_key);
    println!("Token: {:?}", token);
    let as_user = validate_token(&token).await?;
    let user = User::get(&as_user.id).await;
    let user = if let Err(Error::NotFound) = user {
        User::create(as_user.id)
            .await
            .map_err(|_| rapid::errors::Error::InternalError)?
    } else {
        user.map_err(|_| rapid::errors::Error::InternalError)?
    };
    Ok(Box::new(user))
}
static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::new());

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
    let as_user = resp.json::<AsUser>().await.map_err(|_| rapid::errors::Error::InternalError)?;
    Ok(as_user)
}

pub fn check_authenticated(
    clients: Arc<DashMap<String, RpcClient>>,
    id: &str,
) -> Result<Arc<User>> {
    let client = clients.get(id).expect("Failed to get client");
    if let Some(x) = client.get_user::<User>() {
        Ok(x.clone().into())
    } else {
        Err(Error::NotAuthenticated)
    }
}

impl RpcResponder for Error {
    fn into_value(&self) -> Value {
        to_value(self).unwrap()
    }
}
