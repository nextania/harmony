use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use serde::{Deserialize, Serialize};

use crate::authentication::check_authenticated;
use crate::errors::{Error, Result};
use crate::services::database::channels::Channel;
use crate::services::database::users::User;
use crate::services::voice::ActiveCall;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateCallTokenMethod {
    id: String,
    initial_muted: bool,
    initial_deafened: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateCallTokenResponse {
    session_token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RtcAuthorization {
    channel_id: String,
    user_id: String,
}

pub async fn create_call_token(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<CreateCallTokenMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?; // TODO: check rate limit, permissions req'd
    let data = data.into_inner();
    // Check if the user is in the channel
    let user = User::get(&id).await?;
    let Some(mut call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };
    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }
    call.join_user(id.clone()).await?;
    let sdp = call.get_token(&id, &data.sdp).await?;
    Ok(RpcValue(CreateCallTokenResponse { sdp }))
    // Err::<RpcValue<CreateCallTokenResponse>, _>(Error::NoVoiceNodesAvailable)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StartCallMethod {
    id: String,
}

pub async fn start_call(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<StartCallMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let user = User::get(&id).await?;
    let Some(call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };
    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }
    let call = ActiveCall::create(&data.id, &id).await?;
    Ok(RpcValue(StartCallResponse { id: call.id }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StartCallResponse {
    id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndCallMethod {
    id: String,
}

pub async fn end_call(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<EndCallMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let user = User::get(&id).await?;
    let Some(call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };
    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }
    if !channel.is_manager(&id) {
        return Err(Error::MissingPermission);
    }
    let call = ActiveCall::get_in_channel(&data.id).await?;
    if let Some(call) = call {
        call.end().await?;
        Ok(RpcValue(EndCallResponse {}))
    } else {
        Err(Error::NotFound)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndCallResponse {}
