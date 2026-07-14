pub use harmony_types::events::Event;
use serde::{Deserialize, Serialize};
use serde_cbor_2::Value;

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
