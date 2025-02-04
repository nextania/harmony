use std::{sync::Arc, time::Duration};

use async_std::future;
use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use rmp_serde::Serializer;
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated, errors::{Error, Result}, services::database::{channels::Channel, messages::Message, users::User}
};

use super::{Event, NewMessageEvent, RpcApiEvent};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMessagesMethod {
    channel_id: String,
    limit: Option<i64>,
    latest: Option<bool>,
    before: Option<String>,
    after: Option<String>,
}

pub async fn get_messages(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<GetMessagesMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let channel = Channel::get(&data.channel_id).await?;
    let messages = channel
        .get_messages(
            data.limit,
            data.latest,
            data.before.clone(),
            data.after.clone(),
        )
        .await?;
    Ok::<_, Error>(RpcValue(GetMessagesResponse { messages }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMessagesResponse {
    messages: Vec<Message>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageMethod {
    channel_id: String,
    content: String,
}

pub async fn send_message(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<SendMessageMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(clients.clone(), &id)?;
    let data = data.into_inner();
    let trimmed = data.content.trim();
    if trimmed.len() > 4096 {
        return Err(Error::MessageTooLong);
    }
    if trimmed.is_empty() {
        return Err(Error::MessageEmpty);
    }
    let message =
        Message::create(data.channel_id.clone(), user.id.clone(), trimmed.to_owned()).await?;
    for x in clients.clone().iter_mut() {
        // TODO: Check if user is in channel
        if let Some(u) = x.get_user::<User>() {
            println!("Sending message to {}", u.id);
            let mut value_buffer = Vec::new();
            let value = RpcApiEvent {
                event: Event::NewMessage(NewMessageEvent {
                    message: message.clone(),
                    channel_id: data.channel_id.clone(),
                }),
            };
            value
                .serialize(&mut Serializer::new(&mut value_buffer).with_struct_map())
                .unwrap();
            println!("Serialized");
            let y = x.socket.clone();
            future::timeout(Duration::from_millis(5000), async move {
                y.send(async_tungstenite::tungstenite::Message::Binary(
                    value_buffer,
                ))
                .await
            })
            .await
            .unwrap_or(Ok(()))
            .unwrap_or_else(|e| println!("{e:?}"));
        }
    }
    Ok(RpcValue(SendMessageResponse {
        message_id: message.id,
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    message_id: String,
}
