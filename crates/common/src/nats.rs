use std::time::Duration;

use async_nats::jetstream::{self, stream};
use serde::{Deserialize, Serialize};

// event distribution
pub const SUBJECT_EVENTS_DISPATCH: &str = "harmony.events.dispatch";
// pulse -> harmony session connected
pub const SUBJECT_VOICE_CONNECT: &str = "voice.lifecycle.connect";
// pulse -> harmony session disconnected
pub const SUBJECT_VOICE_DISCONNECT: &str = "voice.lifecycle.disconnect";
// node coordination
pub const SUBJECT_NODES_ALL: &str = "voice.nodes.all";

pub fn subject_node(node_instance_id: &str) -> String {
    format!("voice.nodes.{node_instance_id}")
}

pub const STREAM_EVENTS: &str = "EVENTS";
pub const STREAM_VOICE_LIFECYCLE: &str = "VOICE_LIFECYCLE";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventEnvelope {
    pub id: String,
    pub recipients: Vec<String>,
    pub payload: Vec<u8>,
}

pub async fn connect() -> async_nats::Client {
    let url = std::env::var("NATS_URL").expect("NATS_URL must be set");
    let servers: Vec<String> = url.split(',').map(|s| s.trim().to_string()).collect();
    async_nats::ConnectOptions::new()
        .retry_on_initial_connect()
        .connect(servers)
        .await
        .expect("Failed to connect to NATS")
}

pub async fn create_streams(js: &jetstream::Context) {
    js.create_stream(stream::Config {
        name: STREAM_EVENTS.to_string(),
        subjects: vec!["harmony.events.>".to_string()],
        retention: stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(120),
        storage: stream::StorageType::Memory,
        duplicate_window: Duration::from_secs(120),
        num_replicas: 1,
        ..Default::default()
    })
    .await
    .expect("Failed to ensure EVENTS stream");

    js.create_stream(stream::Config {
        name: STREAM_VOICE_LIFECYCLE.to_string(),
        subjects: vec!["voice.lifecycle.>".to_string()],
        retention: stream::RetentionPolicy::Limits,
        max_age: Duration::from_secs(24 * 60 * 60),
        storage: stream::StorageType::File,
        duplicate_window: Duration::from_secs(120),
        num_replicas: 1,
        ..Default::default()
    })
    .await
    .expect("Failed to ensure VOICE_LIFECYCLE stream");
}

pub async fn publish_with_id(
    js: &jetstream::Context,
    subject: impl async_nats::subject::ToSubject,
    id: &str,
    payload: Vec<u8>,
) -> Result<(), async_nats::Error> {
    let mut headers = async_nats::HeaderMap::new();
    headers.insert("Nats-Msg-Id", id);
    js.publish_with_headers(subject, headers, payload.into())
        .await?
        .await?;
    Ok(())
}
