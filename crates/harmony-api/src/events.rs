//! Events that can be received from the Harmony server

use rmpv::Value;
use serde::{Deserialize, Serialize};

use crate::{Channel, Message};

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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    // --- Messages ---
    /// New message received
    NewMessage(NewMessageEvent),
    /// Message was edited
    MessageEdited(MessageEditedEvent),
    /// Message was deleted
    MessageDeleted(MessageDeletedEvent),

    // --- Contacts ---
    /// Contact removed
    RemoveContact(String),
    /// Contact added
    AddContact(String),

    // --- Channels ---
    /// Channel metadata was updated
    ChannelUpdated(ChannelUpdatedEvent),
    /// Channel was deleted
    ChannelDeleted(ChannelDeletedEvent),
    /// A member joined a channel
    MemberJoined(MemberJoinedEvent),
    /// A member left a channel
    MemberLeft(MemberLeftEvent),

    // --- Connection ---
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

    // --- Voice ---
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
}

// --- Event data structs ---

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

/// Event handler trait for processing incoming events
pub trait EventHandler: Send + Sync {
    /// Handle new message event
    fn on_new_message(&self, _event: &NewMessageEvent) {}

    /// Handle message edited event
    fn on_message_edited(&self, _event: &MessageEditedEvent) {}

    /// Handle message deleted event
    fn on_message_deleted(&self, _event: &MessageDeletedEvent) {}

    /// Handle contact removed event
    fn on_remove_contact(&self, _user_id: &str) {}

    /// Handle contact added event
    fn on_add_contact(&self, _user_id: &str) {}

    /// Handle channel updated event
    fn on_channel_updated(&self, _event: &ChannelUpdatedEvent) {}

    /// Handle channel deleted event
    fn on_channel_deleted(&self, _event: &ChannelDeletedEvent) {}

    /// Handle member joined event
    fn on_member_joined(&self, _event: &MemberJoinedEvent) {}

    /// Handle member left event
    fn on_member_left(&self, _event: &MemberLeftEvent) {}

    /// Handle MLS proposal event
    fn on_mls_proposal(&self, _event: &MlsProposalEvent) {}

    /// Handle MLS commit event
    fn on_mls_commit(&self, _event: &MlsCommitEvent) {}

    /// Handle MLS welcome event
    fn on_mls_welcome(&self, _event: &MlsWelcomeEvent) {}

    /// Handle connection established
    fn on_connected(&self) {}

    /// Handle connection lost
    fn on_disconnected(&self) {}

    /// Handle reconnection attempt
    fn on_reconnecting(&self, _attempt: u32, _max_attempts: u32) {}

    /// Handle successful reconnection
    fn on_reconnected(&self) {}

    /// Handle permanent reconnection failure
    fn on_reconnection_failed(&self, _attempts: u32) {}

    /// Handle a user joining a call
    fn on_user_joined_call(
        &self,
        _call_id: &str,
        _user_id: &str,
        _session_id: &str,
        _muted: bool,
        _deafened: bool,
    ) {
    }

    /// Handle a user leaving a call
    fn on_user_left_call(&self, _call_id: &str, _session_id: &str) {}

    /// Handle a user's voice state changing
    fn on_user_voice_state_changed(
        &self,
        _call_id: &str,
        _session_id: &str,
        _muted: bool,
        _deafened: bool,
    ) {
    }
}

/// Default event handler that does nothing
pub struct NoOpEventHandler;

impl EventHandler for NoOpEventHandler {}

/// Simple event handler that prints events to stdout
pub struct PrintEventHandler;

impl EventHandler for PrintEventHandler {
    fn on_new_message(&self, event: &NewMessageEvent) {
        println!(
            "New message in channel {}: {} bytes from {}",
            event.channel_id,
            event.message.content.len(),
            event.message.author_id,
        );
    }

    fn on_message_edited(&self, event: &MessageEditedEvent) {
        println!(
            "Message edited in channel {}: {}",
            event.channel_id, event.message.id,
        );
    }

    fn on_message_deleted(&self, event: &MessageDeletedEvent) {
        println!(
            "Message deleted in channel {}: {}",
            event.channel_id, event.message_id,
        );
    }

    fn on_connected(&self) {
        println!("Connected to Harmony server");
    }

    fn on_disconnected(&self) {
        println!("Disconnected from Harmony server");
    }

    fn on_reconnecting(&self, attempt: u32, max_attempts: u32) {
        println!("Reconnecting... (attempt {} of {})", attempt, max_attempts);
    }

    fn on_reconnected(&self) {
        println!("Successfully reconnected to Harmony server");
    }

    fn on_reconnection_failed(&self, attempts: u32) {
        println!("Failed to reconnect after {} attempts", attempts);
    }

    fn on_user_joined_call(
        &self,
        call_id: &str,
        user_id: &str,
        session_id: &str,
        muted: bool,
        deafened: bool,
    ) {
        println!(
            "User {} joined call {} (session: {}, muted: {}, deafened: {})",
            user_id, call_id, session_id, muted, deafened
        );
    }

    fn on_user_left_call(&self, call_id: &str, session_id: &str) {
        println!("Session {} left call {}", session_id, call_id);
    }

    fn on_user_voice_state_changed(
        &self,
        call_id: &str,
        session_id: &str,
        muted: bool,
        deafened: bool,
    ) {
        println!(
            "Voice state changed in call {} for session {}: muted={}, deafened={}",
            call_id, session_id, muted, deafened
        );
    }
}
