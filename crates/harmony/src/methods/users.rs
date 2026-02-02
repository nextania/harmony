// 1. add friend
// 2. remove friend
// 3. get friends
// 4. get friend requests
// 5. create direct channel
// 6. get direct channels

use rapid::socket::{RpcResponder, RpcState, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{authentication::check_authenticated, errors::Error, services::database::users::User};

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
