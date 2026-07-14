use std::sync::OnceLock;

use async_nats::jetstream::{self, Context};

static CLIENT: OnceLock<async_nats::Client> = OnceLock::new();
static JETSTREAM: OnceLock<Context> = OnceLock::new();

pub async fn connect() {
    let client = common::nats::connect().await;
    let js = jetstream::new(client.clone());
    common::nats::create_streams(&js).await;
    CLIENT.set(client).expect("NATS client already set");
    JETSTREAM.set(js).expect("JetStream context already set");
}

pub fn client() -> &'static async_nats::Client {
    CLIENT.get().expect("NATS client not initialized")
}

pub fn jetstream() -> &'static Context {
    JETSTREAM.get().expect("JetStream context not initialized")
}

pub async fn publish_node_event(subject: String, event: &common::NodeEvent) {
    match serde_cbor_2::to_vec(event) {
        Ok(payload) => {
            if let Err(e) = client().publish(subject, payload.into()).await {
                tracing::error!("Failed to publish node event: {:?}", e);
            }
        }
        Err(e) => tracing::error!("Failed to serialize node event: {:?}", e),
    }
}
