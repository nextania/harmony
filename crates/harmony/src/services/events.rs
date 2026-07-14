use std::time::Duration;

use async_nats::jetstream::consumer::{DeliverPolicy, pull};
use futures_util::StreamExt;
use rapid::socket::RpcClients;
use tokio::{task, time};
use tracing::{error, warn};

use common::nats::{EventEnvelope, SUBJECT_EVENTS_DISPATCH};

use crate::methods::{Event, emit_to_ids};
use crate::services::{nats, redis::INSTANCE_ID};

pub async fn publish(recipients: &[String], event: Event) {
    let payload = match serde_cbor_2::to_vec(&event) {
        Ok(payload) => payload,
        Err(e) => {
            error!("Failed to serialize event for publish: {:?}", e);
            return;
        }
    };
    let id = ulid::Ulid::new().to_string();
    let envelope = EventEnvelope {
        id: id.clone(),
        recipients: recipients.to_vec(),
        payload,
    };
    let body = match serde_cbor_2::to_vec(&envelope) {
        Ok(body) => body,
        Err(e) => {
            error!("Failed to serialize event envelope: {:?}", e);
            return;
        }
    };

    if let Err(e) =
        common::nats::publish_with_id(nats::jetstream(), SUBJECT_EVENTS_DISPATCH, &id, body).await
    {
        error!(
            "Failed to publish event to NATS: {:?}; falling back to local emit",
            e
        );
        if let Some(clients) = crate::RPC_CLIENTS.get() {
            emit_to_ids(clients.clone(), recipients, event);
        }
    }
}

pub async fn publish_one(recipient: &str, event: Event) {
    publish(&[recipient.to_string()], event).await;
}

pub fn spawn_event_subscriber(clients: RpcClients) {
    task::spawn(async move {
        loop {
            if let Err(e) = run_subscriber(&clients).await {
                error!("Event subscriber error: {:?}; recreating consumer in 1s", e);
                time::sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

async fn run_subscriber(clients: &RpcClients) -> Result<(), async_nats::Error> {
    let js = nats::jetstream();
    let stream = js.get_stream(common::nats::STREAM_EVENTS).await?;
    let consumer = stream
        .create_consumer(pull::Config {
            name: Some(format!("events-{}", *INSTANCE_ID)),
            deliver_policy: DeliverPolicy::New,
            ack_policy: async_nats::jetstream::consumer::AckPolicy::None,
            inactive_threshold: Duration::from_secs(30),
            filter_subject: SUBJECT_EVENTS_DISPATCH.to_string(),
            ..Default::default()
        })
        .await?;

    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = message?;
        let envelope: EventEnvelope = match serde_cbor_2::from_slice(&message.payload) {
            Ok(envelope) => envelope,
            Err(e) => {
                warn!("Failed to deserialize event envelope: {:?}", e);
                continue;
            }
        };
        let event: Event = match serde_cbor_2::from_slice(&envelope.payload) {
            Ok(event) => event,
            Err(e) => {
                warn!("Failed to deserialize event payload: {:?}", e);
                continue;
            }
        };
        emit_to_ids(clients.clone(), &envelope.recipients, event);
    }
    Ok(())
}
