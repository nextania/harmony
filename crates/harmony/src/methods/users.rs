
// 1. add friend
// 2. remove friend
// 3. get friends
// 4. get friend requests
// 5. create direct channel
// 6. get direct channels

use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
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

pub async fn add_friend(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<AddFriendMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let user = User::get(&id).await?;
    let friend = User::get(&data.id).await?;
    User::add_friend(&user, &friend.id).await?;
    Ok::<_, Error>(RpcValue(AddFriendResponse {  }))
}

pub async fn add_friend_username(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<AddFriendUsernameMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let user = User::get(&id).await?;
    let friend = User::get_by_username(&data.username).await?;
    User::add_friend(&user, &friend.id).await?;
    Ok::<_, Error>(RpcValue(AddFriendResponse {  }))
}

pub async fn remove_friend(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<AddFriendMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let user = User::get(&id).await?;
    let friend = User::get(&data.id).await?;
    user.remove_friend(&friend.id).await?;
    Ok::<_, Error>(RpcValue(AddFriendResponse {  }))
}

pub async fn get_friends(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    _data: RpcValue<()>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let user = User::get(&id).await?;
    let friends = user.get_affinities().await?;
    Ok::<_, Error>(RpcValue(friends))
}

