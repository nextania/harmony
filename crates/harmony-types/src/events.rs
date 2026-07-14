use serde::{Deserialize, Serialize};

use crate::{channels::Channel, messages::Message, users::RelationshipState};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    // Messages
    NewMessage(NewMessageEvent),
    MessageEdited(MessageEditedEvent),
    MessageDeleted(MessageDeletedEvent),
    // Contacts
    #[serde(rename_all = "camelCase")]
    ContactStateChanged {
        user_id: String,
        state: RelationshipState,
    },
    // Channels
    ChannelUpdated(ChannelUpdatedEvent),
    ChannelDeleted(ChannelDeletedEvent),
    MemberJoined(MemberJoinedEvent),
    MemberLeft(MemberLeftEvent),
    // Voice
    UserJoinedCall(UserJoinedCallEvent),
    UserLeftCall(UserLeftCallEvent),
    UserVoiceStateChanged(UserVoiceStateChangedEvent),
    CallMigrated(CallMigratedEvent),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMessageEvent {
    pub message: Message,
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageEditedEvent {
    pub message: Message,
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeletedEvent {
    pub message_id: String,
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelUpdatedEvent {
    pub channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelDeletedEvent {
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberJoinedEvent {
    pub channel_id: String,
    pub user_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberLeftEvent {
    pub channel_id: String,
    pub user_id: String,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallMigratedEvent {
    pub call_id: String,
    pub server_address: String,
}
