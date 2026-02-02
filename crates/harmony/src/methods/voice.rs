use rapid::socket::{RpcResponder, RpcState, RpcValue};
use serde::{Deserialize, Serialize};

use crate::authentication::check_authenticated;
use crate::errors::Error;
use crate::methods::{Event, UserVoiceStateChangedEvent, emit_to_ids};
use crate::services::database::channels::Channel;
use crate::services::redis::{INSTANCE_ID, get_connection};
use crate::services::voice::ActiveCall;
use pulse_api::{NodeEvent, NodeEventKind};
use redis::AsyncCommands;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateCallTokenMethod {
    id: String,
    initial_muted: bool,
    initial_deafened: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateCallTokenResponse {
    token: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RtcAuthorization {
    channel_id: String,
    user_id: String,
}

pub async fn create_call_token(
    state: RpcState,
    data: RpcValue<CreateCallTokenMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?; // TODO: check rate limit, permissions req'd
    let data = data.into_inner();
    let Some(mut call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };
    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }
    let token = call
        .get_token(&user.id, data.initial_muted, data.initial_deafened)
        .await?;
    // TODO: return all users in the call with their states
    Ok(RpcValue(CreateCallTokenResponse { token }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StartCallMethod {
    id: String,
    preferred_region: Option<pulse_api::Region>,
}

pub async fn start_call(state: RpcState, data: RpcValue<StartCallMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    if ActiveCall::get_in_channel(&data.id).await?.is_some() {
        return Err(Error::AlreadyExists);
    };
    let channel = Channel::get(&data.id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }
    let call = ActiveCall::create(&data.id, &user.id, data.preferred_region).await?;
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

pub async fn end_call(state: RpcState, data: RpcValue<EndCallMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let Some(call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };
    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }
    if !channel.is_manager(&user.id) {
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateVoiceStateMethod {
    id: String,
    muted: Option<bool>,
    deafened: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateVoiceStateResponse {
    muted: bool,
    deafened: bool,
}

pub async fn update_voice_state(
    state: RpcState,
    data: RpcValue<UpdateVoiceStateMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let Some(mut call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };
    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }

    let member_index = call
        .members
        .iter()
        .position(|s| s.user_id == user.id)
        .ok_or(Error::NotFound)?;

    let mut session = call.members[member_index].clone();
    let muted_changed = data
        .muted
        .map_or(false, |new_muted| new_muted != session.muted);
    let deafened_changed = data
        .deafened
        .map_or(false, |new_deafened| new_deafened != session.deafened);
    if !muted_changed && !deafened_changed {
        return Ok(RpcValue(UpdateVoiceStateResponse {
            muted: session.muted,
            deafened: session.deafened,
        }));
    }

    if let Some(new_muted) = data.muted {
        session.muted = new_muted;
    }
    if let Some(new_deafened) = data.deafened {
        session.deafened = new_deafened;
    }

    call.members[member_index] = session.clone();
    call.update().await?;

    let mut redis = get_connection().await;
    let event = NodeEvent {
        id: INSTANCE_ID.clone(),
        event: NodeEventKind::UserStateChange {
            id: session.id.clone(),
            muted: session.muted,
            deafened: session.deafened,
        },
    };
    redis
        .publish::<&str, NodeEvent, ()>("nodes", event)
        .await?;

    let member_user_ids: Vec<String> = call.members.iter().map(|s| s.user_id.clone()).collect();

    emit_to_ids(
        state.clients(),
        &member_user_ids,
        Event::UserVoiceStateChanged(UserVoiceStateChangedEvent {
            call_id: call.id.clone(),
            session_id: session.id.clone(),
            muted: session.muted,
            deafened: session.deafened,
        }),
    );

    Ok(RpcValue(UpdateVoiceStateResponse {
        muted: session.muted,
        deafened: session.deafened,
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCallMembersMethod {
    id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CallMember {
    user_id: String,
    session_id: String,
    muted: bool,
    deafened: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCallMembersResponse {
    members: Vec<CallMember>,
}

pub async fn get_call_members(
    state: RpcState,
    data: RpcValue<GetCallMembersMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();

    let Some(call) = ActiveCall::get_in_channel(&data.id).await? else {
        return Err(Error::NotFound);
    };

    let channel = Channel::get(&call.channel_id).await?;
    if !user.in_channel(&channel).await? {
        return Err(Error::NotFound);
    }

    let members: Vec<CallMember> = call
        .members
        .iter()
        .map(|session| CallMember {
            user_id: session.user_id.clone(),
            session_id: session.id.clone(),
            muted: session.muted,
            deafened: session.deafened,
        })
        .collect();

    Ok(RpcValue(GetCallMembersResponse { members }))
}
