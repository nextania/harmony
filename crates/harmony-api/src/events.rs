//! Events that can be received from the Harmony server

use rmpv::Value;
use serde::{Deserialize, Serialize};

/// Events that can be received from the server
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcMessageS2C {
    /// Initial connection information
    #[serde(rename_all = "camelCase")]
    Hello {
        public_key: Vec<u8>,
    },
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
    /// New message received
    // NewMessage(NewMessageEvent),
    /// Friend removed
    // RemoveFriend(String),
    /// Friend added
    // AddFriend(String),
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

/// Event handler trait for processing incoming events
pub trait EventHandler: Send + Sync {
    /// Handle new message event
    /// fn on_new_message(&self, event: &NewMessageEvent) {}
    ///
    /// Handle friend removed event
    /// fn on_remove_friend(&self, user_id: &str) {}
    ///
    /// Handle friend added event
    /// fn on_add_friend(&self, user_id: &str) {}

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
