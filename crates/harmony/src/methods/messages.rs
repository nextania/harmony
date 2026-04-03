use harmony_types::messages::{
    DeleteMessageMethod, DeleteMessageResponse, EditMessageMethod, EditMessageResponse,
    GetMessagesMethod, GetMessagesResponse, SendMessageMethod, SendMessageResponse,
};
use rapid::socket::{RpcResponder, RpcState, RpcValue};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    methods::{Event, MessageDeletedEvent, MessageEditedEvent, NewMessageEvent, emit_to_ids},
    services::database::{
        channels::{Channel, EncryptionHint},
        messages::Message,
        users::User,
    },
};

pub async fn get_messages(state: RpcState, data: RpcValue<GetMessagesMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    let channel = Channel::get(&data.channel_id).await?;
    if !channel.is_member(&user.id) {
        return Err(Error::NotInChannel);
    }
    if let Channel::GroupChannel {
        encryption_hint: EncryptionHint::Mls,
        ..
    } = &channel
    {
        return Err(Error::Unimplemented);
    }
    let messages = channel
        .get_messages(
            data.limit,
            data.latest,
            data.before.clone(),
            data.after.clone(),
        )
        .await?;
    Ok::<_, Error>(RpcValue(GetMessagesResponse {
        messages: messages.into_iter().map(|m| m.into()).collect(),
    }))
}

pub async fn send_message(state: RpcState, data: RpcValue<SendMessageMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    if data.content.len() > 65536 {
        return Err(Error::MessageTooLong);
    }
    if data.content.is_empty() {
        return Err(Error::MessageEmpty);
    }
    let channel = Channel::get(&data.channel_id).await?;
    if !channel.is_member(&user.id) {
        return Err(Error::NotInChannel);
    }
    if let Channel::PrivateChannel {
        initiator_id,
        target_id,
        ..
    } = &channel
    {
        let other_id = if initiator_id == &user.id {
            target_id
        } else {
            initiator_id
        };
        if (user.can_dm(&User::get(other_id).await?).await?).is_none() {
            return Err(Error::InvalidTarget);
        }
    }
    let is_mls = matches!(
        &channel,
        Channel::GroupChannel {
            encryption_hint: EncryptionHint::Mls,
            ..
        }
    );

    let message = if is_mls {
        Message::ephemeral(&data.channel_id, &user.id, &data.content).await?
    } else {
        Message::create(&channel, &user.id, &data.content).await?
    };

    let member_ids = channel.member_ids();
    emit_to_ids(
        state.clients(),
        &member_ids,
        Event::NewMessage(NewMessageEvent {
            message: message.clone(),
            channel_id: data.channel_id,
        }),
    );

    Ok(RpcValue(SendMessageResponse {
        message: message.into(),
    }))
}

pub async fn edit_message(state: RpcState, data: RpcValue<EditMessageMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    if data.content.len() > 65536 {
        return Err(Error::MessageTooLong);
    }
    if data.content.is_empty() {
        return Err(Error::MessageEmpty);
    }
    let message = Message::get(&data.message_id).await?;
    if message.author_id != user.id {
        return Err(Error::MissingPermission);
    }
    let updated = message.edit(data.content).await?;
    let channel = Channel::get(&updated.channel_id).await?;
    let member_ids = channel.member_ids();
    emit_to_ids(
        state.clients(),
        &member_ids,
        Event::MessageEdited(MessageEditedEvent {
            message: updated.clone(),
            channel_id: updated.channel_id.clone(),
        }),
    );
    Ok(RpcValue(EditMessageResponse {
        message: updated.into(),
    }))
}

pub async fn delete_message(
    state: RpcState,
    data: RpcValue<DeleteMessageMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    let message = Message::get(&data.message_id).await?;
    let channel = Channel::get(&message.channel_id).await?;
    let is_author = message.author_id == user.id;
    let is_manager = channel.is_manager(&user.id);
    if !is_author && !is_manager {
        return Err(Error::MissingPermission);
    }
    let deleted = message.delete().await?;
    let member_ids = channel.member_ids();
    emit_to_ids(
        state.clients(),
        &member_ids,
        Event::MessageDeleted(MessageDeletedEvent {
            message_id: deleted.id.clone(),
            channel_id: deleted.channel_id.clone(),
        }),
    );
    Ok(RpcValue(DeleteMessageResponse {}))
}
