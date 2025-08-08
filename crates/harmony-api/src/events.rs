//! Events that can be received from the Harmony server

use rmpv::Value;
use serde::{Deserialize, Serialize};

/// Events that can be received from the server
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcApiEvent {
    /// Initial connection information
    #[serde(rename_all = "camelCase")]
    Hello {
        public_key: Vec<u8>,
    },
    Identify {},
    Heartbeat {},
    Message {
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
}
