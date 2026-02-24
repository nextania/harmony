use rapid::socket::{RpcResponder, RpcState, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    methods::{
        ChannelDeletedEvent, ChannelUpdatedEvent, Event, MemberLeftEvent, emit_to_ids
    },
    services::database::{channels::{Channel, ChannelMemberRole, EncryptionHint}, messages::Message},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelMethod {
    id: String,
}

pub async fn get_channel(state: RpcState, data: RpcValue<GetChannelMethod>) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let channel = Channel::get(&data.id).await?;
    if !channel.is_member(&user.id) {
        return Err(Error::NotInChannel);
    }
    Ok(RpcValue(GetChannelResponse { channel }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelResponse {
    channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelsMethod {}

pub async fn get_channels(state: RpcState, _: RpcValue<GetChannelsMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let channels = user.get_channels().await?;
    Ok::<_, Error>(RpcValue(GetChannelsResponse { channels }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelsResponse {
    channels: Vec<Channel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateChannelMethod {
    channel: ChannelInformation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelInformation {
    #[serde(rename_all = "camelCase")]
    PrivateChannel {
        target_id: String,
    },
    #[serde(rename_all = "camelCase")]
    GroupChannel {
        metadata: Vec<u8>,
        encryption_hint: EncryptionHint,
    },
}

pub async fn create_channel(
    state: RpcState,
    data: RpcValue<CreateChannelMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let channel = match data.channel {
        ChannelInformation::PrivateChannel { target_id } => {
            // check if the user is trying to create a private channel with themselves
            if target_id == user.id {
                return Err(Error::InvalidTarget);
            }
            // check if the user's relationship with the target allows for creating a private channel
            let target = crate::services::database::users::User::get(&target_id).await?;
            if !user.can_dm(&target).await? {
                return Err(Error::InvalidTarget);
            }
            Channel::create_private(user.id.clone(), target_id).await?
        }
        ChannelInformation::GroupChannel {
            metadata,
            encryption_hint,
        } => {
            if let EncryptionHint::Mls = encryption_hint {
                // FIXME: MLS implementation is quite a bit of work
                // particularly, we need to track a key package for every device the user is logged in on
                // each time a user logs in, a key package needs to be uploaded 
                // and then verified and approved by another device by generating commits
                return Err(Error::Unimplemented);
            }
            Channel::create_group(user.id.clone(), metadata, encryption_hint).await?
        },
    };
    Ok::<_, Error>(RpcValue(CreateChannelResponse { channel }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChannelResponse {
    channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditChannelMethod {
    channel_id: String,
    metadata: Vec<u8>,
}

pub async fn edit_channel(
    state: RpcState,
    data: RpcValue<EditChannelMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let channel = Channel::get(&data.channel_id).await?;
    if !channel.is_manager(&user.id) {
        return Err(Error::MissingPermission);
    }
    channel.update_metadata(data.metadata).await?;
    let updated = Channel::get(&data.channel_id).await?;
    let member_ids = updated.member_ids();
    emit_to_ids(
        state.clients(),
        &member_ids,
        Event::ChannelUpdated(ChannelUpdatedEvent {
            channel: updated.clone(),
        }),
    );
    Ok(RpcValue(EditChannelResponse { channel: updated }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditChannelResponse {
    channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteChannelMethod {
    channel_id: String,
}

pub async fn delete_channel(
    state: RpcState,
    data: RpcValue<DeleteChannelMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let channel = Channel::get(&data.channel_id).await?;
    if !channel.is_manager(&user.id) {
        return Err(Error::MissingPermission);
    }
    let member_ids = channel.member_ids();
    channel.delete().await?;
    // also delete all messages associated with the channel
    Message::delete_in(&data.channel_id).await?;
    emit_to_ids(
        state.clients(),
        &member_ids,
        Event::ChannelDeleted(ChannelDeletedEvent {
            channel_id: data.channel_id,
        }),
    );
    Ok(RpcValue(DeleteChannelResponse {}))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteChannelResponse {}

// --- LEAVE_CHANNEL ---

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveChannelMethod {
    channel_id: String,
}

pub async fn leave_channel(
    state: RpcState,
    data: RpcValue<LeaveChannelMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let channel = Channel::get(&data.channel_id).await?;
    match &channel {
        Channel::PrivateChannel { .. } => {
            // private channels cannot be left - they exist as long as both users exist
            return Err(Error::MissingPermission);
        }
        Channel::GroupChannel { members, .. } => {
            if !channel.is_member(&user.id) {
                return Err(Error::NotInChannel);
            }
            // if the user is a manager and is the last manager, prevent leaving
            let is_manager = members
                .iter()
                .any(|m| m.id == user.id && m.role == ChannelMemberRole::Manager);
            if is_manager && channel.manager_count() <= 1 && members.len() > 1 {
                return Err(Error::LastManager);
            }
            channel.remove_member(&user.id).await?;
            if members.len() <= 1 {
                channel.delete().await?;
            } else {
                let remaining: Vec<String> = members
                    .iter()
                    .filter(|m| m.id != user.id)
                    .map(|m| m.id.clone())
                    .collect();
                emit_to_ids(
                    state.clients(),
                    &remaining,
                    Event::MemberLeft(MemberLeftEvent {
                        channel_id: data.channel_id.clone(),
                        user_id: user.id.clone(),
                    }),
                );
            }
        }
    }
    Ok(RpcValue(LeaveChannelResponse {}))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveChannelResponse {}
