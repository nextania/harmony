use std::{
    sync::{OnceLock, atomic::Ordering},
    time::Duration,
};

use async_nats::jetstream::{self, Context};
use common::{
    NodeDescription, NodeEvent, NodeEventKind,
    nats::{SUBJECT_NODES_ALL, subject_node},
};
use futures::StreamExt;
use pulse_types::{ControlS2C, MediaHint};
use tokio::{task, time};

use crate::environment::{PUBLIC_ADDRESS, REGION};
use crate::redis::INSTANCE_ID;

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

fn description_event() -> NodeEvent {
    NodeEvent {
        event: NodeEventKind::Description(NodeDescription {
            region: *REGION,
            server_address: PUBLIC_ADDRESS.clone(),
        }),
        id: INSTANCE_ID.clone(),
    }
}

async fn publish_node(subject: String, event: &NodeEvent) {
    let payload = match serde_cbor_2::to_vec(event) {
        Ok(payload) => payload,
        Err(e) => {
            error!("Failed to serialize node event: {:?}", e);
            return;
        }
    };
    if let Err(e) = client().publish(subject, payload.into()).await {
        error!("Failed to publish node event: {:?}", e);
    }
}

pub fn listen() {
    // node registration/inbound commands
    task::spawn(async move {
        let nats = client();
        let mut broadcast = match nats.subscribe(SUBJECT_NODES_ALL).await {
            Ok(sub) => sub,
            Err(e) => {
                error!("Failed to subscribe to {}: {:?}", SUBJECT_NODES_ALL, e);
                return;
            }
        };
        let directed_subject = subject_node(&INSTANCE_ID);
        let mut directed = match nats.subscribe(directed_subject.clone()).await {
            Ok(sub) => sub,
            Err(e) => {
                error!("Failed to subscribe to {}: {:?}", directed_subject, e);
                return;
            }
        };

        // announce presence on startup
        publish_node(SUBJECT_NODES_ALL.to_string(), &description_event()).await;

        loop {
            let msg = tokio::select! {
                Some(msg) = broadcast.next() => msg,
                Some(msg) = directed.next() => msg,
                else => break,
            };
            let payload: NodeEvent = match serde_cbor_2::from_slice(&msg.payload) {
                Ok(payload) => payload,
                Err(e) => {
                    error!("Failed to deserialize node event: {:?}", e);
                    continue;
                }
            };
            if payload.id == *INSTANCE_ID {
                continue;
            }
            debug!("Received: {:?}", payload);
            handle_node_event(payload).await;
        }
    });

    // heartbeat ping
    task::spawn(async move {
        loop {
            let event = NodeEvent {
                event: NodeEventKind::Ping,
                id: INSTANCE_ID.clone(),
            };
            publish_node(SUBJECT_NODES_ALL.to_string(), &event).await;
            time::sleep(Duration::from_secs(5)).await;
        }
    });
}

async fn handle_node_event(payload: NodeEvent) {
    match payload {
        NodeEvent {
            event: NodeEventKind::Query,
            ..
        } => {
            publish_node(SUBJECT_NODES_ALL.to_string(), &description_event()).await;
        }

        NodeEvent {
            event:
                NodeEventKind::UserStateChange {
                    id,
                    muted,
                    deafened,
                },
            ..
        } => {
            if let Some(session) = crate::wt::GLOBAL_SESSIONS.get(&id) {
                let session_id = session.id.clone();
                let call_id = session.call_id.clone();

                session.can_speak.store(!muted, Ordering::SeqCst);
                session.can_listen.store(!deafened, Ordering::SeqCst);

                if muted && let Some(call) = crate::wt::GLOBAL_CALLS.get(&call_id) {
                    for track in session.producers.iter() {
                        if matches!(track.media_hint, MediaHint::Audio) {
                            for member_id in call.members.iter() {
                                let member_key: &String = member_id.key();
                                if member_key == &session_id {
                                    continue;
                                }
                                if let Some(member_session) =
                                    crate::wt::GLOBAL_SESSIONS.get(member_key)
                                {
                                    let _ = member_session.message_tx.send(
                                        ControlS2C::TrackUnavailable {
                                            id: track.id.clone(),
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        NodeEvent {
            event: NodeEventKind::UserDisconnect { id, .. },
            ..
        } => {
            if let Some(session) = crate::wt::GLOBAL_SESSIONS.get(&id) {
                let _ = session
                    .message_tx
                    .send(ControlS2C::Disconnected { reconnect: None });

                session.close("User disconnected by server");
            }
        }

        NodeEvent {
            event:
                NodeEventKind::UserMoved {
                    id,
                    target_server,
                    target_token,
                },
            ..
        } => {
            if let Some(session) = crate::wt::GLOBAL_SESSIONS.get(&id) {
                let _ = session.message_tx.send(ControlS2C::Disconnected {
                    reconnect: Some((target_server, target_token)),
                });

                session.close("User moved to another server");
            }
        }

        NodeEvent {
            event: NodeEventKind::CallEnded { call_id },
            ..
        } => {
            if let Some((_, call)) = crate::wt::GLOBAL_CALLS.remove(&call_id) {
                for member_id in call.members.iter() {
                    let member_key: &String = member_id.key();
                    if let Some(session) = crate::wt::GLOBAL_SESSIONS.get(member_key) {
                        let _ = session
                            .message_tx
                            .send(ControlS2C::Disconnected { reconnect: None });

                        session.close("Call ended");
                    }
                }
            }
        }

        _ => {}
    }
}
