//! Events that can be received from the Harmony server

use rmpv::Value;
use serde::{Deserialize, Serialize};

use crate::{Channel, Message, RelationshipState};

/// Events that can be received from the server
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcMessageS2C {
    /// Initial connection information
    #[serde(rename_all = "camelCase")]
    Hello {},
    Identify {},
    Heartbeat {},
    Message {
        id: String,
        data: Value,
    },
    Event {
        event: Value,
    },
}

// TODO: use one enum
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    /// New message received
    NewMessage(NewMessageEvent),
    /// Message was edited
    MessageEdited(MessageEditedEvent),
    /// Message was deleted
    MessageDeleted(MessageDeletedEvent),

    /// Contact relationship state changed
    #[serde(rename_all = "camelCase")]
    ContactStateChanged {
        user_id: String,
        state: RelationshipState,
    },

    /// Channel metadata was updated
    ChannelUpdated(ChannelUpdatedEvent),
    /// Channel was deleted
    ChannelDeleted(ChannelDeletedEvent),
    /// A member joined a channel
    MemberJoined(MemberJoinedEvent),
    /// A member left a channel
    MemberLeft(MemberLeftEvent),

    /// A user joined a call
    #[serde(rename_all = "camelCase")]
    UserJoinedCall {
        call_id: String,
        user_id: String,
        session_id: String,
        muted: bool,
        deafened: bool,
    },
    /// A user left a call
    #[serde(rename_all = "camelCase")]
    UserLeftCall { call_id: String, session_id: String },
    /// A user's voice state changed (muted/deafened)
    #[serde(rename_all = "camelCase")]
    UserVoiceStateChanged {
        call_id: String,
        session_id: String,
        muted: bool,
        deafened: bool,
    },

    /// Connection was established
    Connected,
    /// Connection was lost
    Disconnected,
    /// Reconnection attempt started
    Reconnecting { attempt: u32, max_attempts: u32 },
    /// Reconnection successful
    Reconnected,
    /// Reconnection failed permanently
    ReconnectionFailed { attempts: u32 },
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
pub struct MlsProposalEvent {
    pub channel_id: String,
    pub proposal: Vec<u8>,
    pub epoch: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MlsCommitEvent {
    pub channel_id: String,
    pub commit: Vec<u8>,
    pub epoch: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MlsWelcomeEvent {
    pub channel_id: String,
    pub welcome: Vec<u8>,
}
