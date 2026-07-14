use serde::{Deserialize, Serialize};
use serde_cbor_2::Value;

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
        ok: bool,
        data: Value,
    },
    Event {
        event: Value,
    },
}

/// A server-originated event.
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
    /// The call was moved to a different voice node; reconnect to the new
    /// server to stay in the call.
    #[serde(rename_all = "camelCase")]
    CallMigrated {
        call_id: String,
        server_address: String,
    },
}

/// Client-generated transport lifecycle events.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LifecycleEvent {
    /// Connection was established (and authenticated)
    Connected,
    /// Connection was lost
    Disconnected,
    /// Reconnection attempt started
    Reconnecting { attempt: u32, max_attempts: u32 },
    /// Reconnection successful (and re-authenticated)
    Reconnected,
    /// Reconnection failed permanently
    ReconnectionFailed { attempts: u32 },
}

/// The unified event delivered to consumers of the client event stream.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum ClientEvent {
    Event(Event),
    Lifecycle(LifecycleEvent),
}

impl From<Event> for ClientEvent {
    fn from(event: Event) -> Self {
        ClientEvent::Event(event)
    }
}

impl From<LifecycleEvent> for ClientEvent {
    fn from(event: LifecycleEvent) -> Self {
        ClientEvent::Lifecycle(event)
    }
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
