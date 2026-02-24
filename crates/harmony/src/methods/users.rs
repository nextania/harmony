// 1. add friend
// 2. remove friend
// 3. get friends
// 4. get friend requests
// 5. create direct channel
// 6. get direct channels

use rapid::socket::{RpcResponder, RpcState, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{authentication::check_authenticated, errors::Error, services::database::users::{Presence, User}};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddFriendMethod {
    id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddFriendUsernameMethod {
    username: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AddFriendResponse {}

pub async fn add_friend(state: RpcState, data: RpcValue<AddFriendMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get(&data.id).await?;
    User::add_contact(&user, &friend.id).await?;
    Ok::<_, Error>(RpcValue(AddFriendResponse {}))
}

pub async fn add_friend_username(
    state: RpcState,
    data: RpcValue<AddFriendUsernameMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get_by_username(&data.username).await?;
    User::add_contact(&user, &friend.id).await?;
    Ok::<_, Error>(RpcValue(AddFriendResponse {}))
}

pub async fn remove_friend(state: RpcState, data: RpcValue<AddFriendMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get(&data.id).await?;
    user.remove_contact(&friend.id).await?;
    Ok::<_, Error>(RpcValue(AddFriendResponse {}))
}

pub async fn get_friends(state: RpcState, _data: RpcValue<()>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let contacts = user.get_contacts().await?;
    Ok::<_, Error>(RpcValue(contacts))
}

/// Get the current authenticated user data and keys
/// This method should be used immediately after authentication
pub async fn get_current_user(state: RpcState, _data: RpcValue<()>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    Ok::<_, Error>(RpcValue(CurrentUserResponse {
        id: user.id.clone(),
        public_key: user.key_package.as_ref().map(|kp| kp.public_key.clone()),
        encrypted_keys: user.key_package.as_ref().map(|kp| kp.encrypted_keys.clone()),
        presence: user.presence.clone(),
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserResponse {
    pub id: String,
    pub public_key: Option<Vec<u8>>,
    pub encrypted_keys: Option<Vec<u8>>,
    pub presence: Presence,
}
