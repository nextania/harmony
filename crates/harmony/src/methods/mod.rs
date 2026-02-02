use rapid::socket::RpcClients;
use serde::{Deserialize, Serialize};

use crate::services::database::{messages::Message, users::User};

pub mod channels;
pub mod invites;
pub mod messages;
pub mod users;
pub mod voice;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    // WebRTC: 10-19
    NewMessage(NewMessageEvent),
    RemoveFriend(String),
    AddFriend(String),
    UserJoinedCall(UserJoinedCallEvent),
    UserLeftCall(UserLeftCallEvent),
    UserVoiceStateChanged(UserVoiceStateChangedEvent),
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserJoinedCallEvent {
    pub call_id: String,
    pub user_id: String,
    pub session_id: String,
    pub muted: bool,
    pub deafened: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLeftCallEvent {
    pub call_id: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserVoiceStateChangedEvent {
    pub call_id: String,
    pub session_id: String,
    pub muted: bool,
    pub deafened: bool,
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

pub fn emit_to_id(clients: RpcClients, user_id: &str, event: Event) {
    clients.emit_by(
        event,
        |client| {
            let i = client.get_user::<User>().map(|u| u.id.clone());
            i == Some(user_id.to_owned())
        },
    );
}

pub fn emit_to_ids(
    clients: RpcClients,
    user_ids: &[String],
    event: Event,
) {
    clients.emit_by(
        event,
        |client| {
            let i = client.get_user::<User>().map(|u| u.id.clone());
            if let Some(user_id) = i {
                user_ids.contains(&user_id)
            } else {
                false
            }
        },
    );
}
