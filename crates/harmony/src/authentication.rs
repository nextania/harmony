use std::{any::Any, sync::Arc};

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder};
use rmpv::{Value, ext::to_value};
use serde::Deserialize;

use crate::{
    errors::{Error, Result},
    services::database::users::User,
};

#[derive(Deserialize)]
struct UserJwt {
    // TODO: Find the other properties
    id: String,
    issued_at: u128,
    expires_at: u128,
}

// Important: This only accepts a token and will not sign a token.
// The token is to be obtained from a separate login server
// (e.g. AS)
// TODO: fetch real valid token information from AS
pub async fn authenticate(token: String) -> rapid::errors::Result<Box<dyn Any + Send + Sync>> {
    // println!("Public key: {:?}", self.public_key);
    println!("Token: {:?}", token);
    // TODO: validate the token
    let user = User::get(&token).await;
    let user = if let Err(Error::NotFound) = user {
        User::create(token)
            .await
            .map_err(|_| rapid::errors::Error::InternalError)?
    } else {
        user.map_err(|_| rapid::errors::Error::InternalError)?
    };
    Ok(Box::new(user))
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
