use std::{any::Any, collections::HashSet, sync::Arc};

use dashmap::DashMap;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use rapid::socket::{RpcClient, RpcResponder};
use rmpv::{ext::to_value, Value};
use serde::Deserialize;

use crate::{errors::{Error, Result}, services::{database::users::User, environment::JWT_SECRET}};

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
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims = HashSet::new();
    validation.validate_exp = false;
    let token_message = decode::<UserJwt>(
        &token,
        &DecodingKey::from_secret(JWT_SECRET.as_ref()),
        &validation,
    )
    .map_err(|_| rapid::errors::Error::InvalidToken)?;
    let time = chrono::Utc::now().timestamp_millis() as u128;
    if time > token_message.claims.expires_at {
        return Err(rapid::errors::Error::InvalidToken);
    }
    let user = User::get(&token_message.claims.id).await;
    let user = if let Err(Error::NotFound) = user {
        User::create(token_message.claims.id).await.map_err(|_| rapid::errors::Error::InternalError)?
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
