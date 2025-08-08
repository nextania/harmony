//! Events that can be received from the Harmony server

use serde::{Deserialize, Serialize};
use crate::models::Message;

/// Events that can be received from the server
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    /// A new message was sent to a channel
    NewMessage(NewMessageEvent),
    /// A friend was removed
    RemoveFriend(String),
    /// A friend was added
    AddFriend(String),
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

/// Event wrapper that matches the server's format
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcApiEvent {
    #[serde(flatten)]
    pub event: Event,
}

/// Hello event sent when connecting
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloEvent {
    pub public_key: Vec<u8>,
    pub request_ids: Vec<String>,
}

/// New message event
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMessageEvent {
    pub message: Message,
    pub channel_id: String,
}

/// Event handler trait for processing incoming events
pub trait EventHandler: Send + Sync {
    /// Handle a new message event
    fn on_new_message(&self, _event: &NewMessageEvent) {}
    
    /// Handle a friend removal event
    fn on_remove_friend(&self, _user_id: &str) {}
    
    /// Handle a friend addition event
    fn on_add_friend(&self, _user_id: &str) {}
    
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
    
    /// Handle any event (called for all events)
    fn on_event(&self, _event: &Event) {}
}

/// Default event handler that does nothing
pub struct NoOpEventHandler;

impl EventHandler for NoOpEventHandler {}

/// Simple event handler that prints events to stdout
pub struct PrintEventHandler;

impl EventHandler for PrintEventHandler {
    fn on_new_message(&self, event: &NewMessageEvent) {
        println!("New message in channel {}: {}", event.channel_id, event.message.content);
    }
    
    fn on_remove_friend(&self, user_id: &str) {
        println!("Friend removed: {}", user_id);
    }
    
    fn on_add_friend(&self, user_id: &str) {
        println!("Friend added: {}", user_id);
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
}
