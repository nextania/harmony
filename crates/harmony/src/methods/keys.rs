use rapid::socket::{RpcResponder, RpcState, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    services::database::users::User,
};


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyPackageMethod {
    /// x25519 public key for persistent encryption (DMs / persistent group channels)
    pub public_key: Vec<u8>,
    /// encrypted private key material (encrypted client-side, opaque to server)
    pub encrypted_keys: Vec<u8>,
}

pub async fn set_key_package(
    state: RpcState,
    data: RpcValue<SetKeyPackageMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    user.set_key_package(data.public_key, data.encrypted_keys)
        .await?;
    Ok::<_, Error>(RpcValue(SetKeyPackageResponse {}))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetKeyPackageResponse {}


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUserMethod {
    user_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProfile {
    pub id: String,
    /// x25519 public key (for persistent encryption). None if the user hasn't uploaded keys yet.
    pub public_key: Option<Vec<u8>>,
}

pub async fn get_user(state: RpcState, data: RpcValue<GetUserMethod>) -> impl RpcResponder {
    let _user = check_authenticated(&state)?;
    let data = data.into_inner();
    let target = User::get(&data.user_id).await?;
    let public_key = target.key_package.as_ref().map(|kp| kp.public_key.clone());
    Ok::<_, Error>(RpcValue(GetUserResponse {
        user: UserProfile {
            id: target.id,
            public_key,
        },
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetUserResponse {
    user: UserProfile,
}
