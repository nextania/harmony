use std::sync::Arc;

use dashmap::{DashMap, mapref::multiple::RefMulti};
use rapid::socket::{RpcClient, emit_one};
use serde::{Deserialize, Serialize};

use crate::services::database::{messages::Message, users::User};

pub mod channels;
pub mod invites;
pub mod messages;
pub mod users;
pub mod webrtc;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    // WebRTC: 10-19
    NewMessage(NewMessageEvent),
    RemoveFriend(String),
    AddFriend(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloEvent {
    pub(crate) public_key: Vec<u8>,
    pub(crate) request_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMessageEvent {
    message: Message,
    channel_id: String,
}

pub enum CreateChannelType {
    PrivateChannel {
        peer_id: String,
        scope_id: String,
    },
    GroupChannel {
        name: String,
        description: String,
        members: Vec<String>,
        scope_id: String,
    },
    InformationChannel {
        name: String,
        description: String,
        nexus_id: String,
        scope_id: String,
    },
    TextChannel {
        name: String,
        description: String,
        nexus_id: String,
        scope_id: String,
    },
}

pub fn emit_to_id(clients: Arc<DashMap<String, RpcClient>>, user_id: &str, event: Event) {
    let client: Vec<RefMulti<'_, String, RpcClient>> = clients
        .iter()
        .filter(|client| {
            let i = client.get_user::<User>().map(|u| u.id.clone());
            i == Some(user_id.to_owned())
        })
        .collect();
    for client in client {
        emit_one(client.value(), event.clone());
    }
}
