use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Routes incoming media datagrams to per-track receivers.
///
/// When the client subscribes to a track via `consume_track`, a per-track channel is
/// created. Incoming `WtTrackData` datagrams are demultiplexed by track ID and forwarded
/// to the appropriate receiver. The consumer gets an `UnboundedReceiver<Vec<u8>>` for
/// each track they subscribe to.
#[derive(Clone)]
pub struct MediaRouter {
    senders: Arc<DashMap<String, mpsc::UnboundedSender<Vec<u8>>>>,
}

impl MediaRouter {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(DashMap::new()),
        }
    }

    /// Register a new track and return a receiver for its media data.
    ///
    /// Called before sending `StartConsume` to the server so that no datagrams
    /// arriving between the request and confirmation are missed.
    pub fn subscribe(&self, track_id: &str) -> mpsc::UnboundedReceiver<Vec<u8>> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.senders.insert(track_id.to_string(), tx);
        rx
    }

    /// Remove a track's media channel.
    ///
    /// Called when consumption stops (`ConsumeStopped`, `TrackUnavailable`,
    /// or explicit `stop_consuming`).
    pub fn unsubscribe(&self, track_id: &str) {
        self.senders.remove(track_id);
    }

    /// Dispatch incoming media data to the appropriate per-track receiver.
    ///
    /// If no subscriber exists for the track ID, the data is silently dropped.
    pub fn dispatch(&self, track_id: &str, data: Vec<u8>) {
        if let Some(sender) = self.senders.get(track_id) {
            if sender.send(data).is_err() {
                drop(sender);
                self.senders.remove(track_id);
            }
        }
    }

    /// Check whether a track is currently subscribed.
    pub fn is_subscribed(&self, track_id: &str) -> bool {
        self.senders.contains_key(track_id)
    }
}

impl Default for MediaRouter {
    fn default() -> Self {
        Self::new()
    }
}
