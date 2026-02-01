use std::time::Duration;

use futures::StreamExt;
use once_cell::sync::{Lazy, OnceCell};
use pulse_api::{NodeDescription, NodeEvent, NodeEventKind};
use redis::{AsyncCommands, Client, aio::MultiplexedConnection};
use tokio::{task, time};
use ulid::Ulid;

use crate::{
    environment::{REDIS_URI, REGION},
};

static REDIS: OnceCell<Client> = OnceCell::new();
pub static INSTANCE_ID: Lazy<String> = Lazy::new(|| Ulid::new().to_string());

pub fn connect() {
    let client = Client::open(&**REDIS_URI).expect("Failed to connect");
    REDIS.set(client).expect("Failed to set client");
}

pub fn get_client() -> &'static Client {
    REDIS.get().expect("Failed to get client")
}

pub async fn get_connection() -> MultiplexedConnection {
    get_client()
        .get_multiplexed_async_connection()
        .await
        .expect("Failed to get connection")
}

pub async fn get_pubsub() -> redis::aio::PubSub {
    get_client()
        .get_async_pubsub()
        .await
        .expect("Failed to get connection")
}

pub async fn listen() -> () {
    let mut pubsub = get_pubsub().await;
    pubsub
        .subscribe("nodes")
        .await
        .expect("Failed to subscribe");
    let mut connection = get_connection().await;
    connection
        .publish::<&str, NodeEvent, NodeEvent>(
            "nodes",
            NodeEvent {
                event: NodeEventKind::Description(NodeDescription { region: *REGION }),
                id: INSTANCE_ID.clone(),
            },
        )
        .await;
    let mut c = connection.clone();
    let i = INSTANCE_ID.clone();
    task::spawn(async move {
        loop {
            c.publish::<&str, NodeEvent, NodeEvent>(
                "nodes",
                NodeEvent {
                    event: NodeEventKind::Ping,
                    id: i.clone(),
                },
            )
            .await;
            time::sleep(Duration::from_secs(5)).await;
        }
    });
    while let Some(msg) = pubsub.on_message().next().await {
        let payload: NodeEvent = msg.get_payload().unwrap();
        if payload.id == *INSTANCE_ID {
            continue;
        }
        println!("Received: {:?}", payload);
        match payload {
            NodeEvent {
                event: NodeEventKind::Query,
                ..
            } => {
                connection
                    .publish::<&str, NodeEvent, ()>(
                        "nodes",
                        NodeEvent {
                            event: NodeEventKind::Description(NodeDescription { region: *REGION }),
                            id: INSTANCE_ID.clone(),
                        },
                    )
                    .await
                    .expect("Failed to publish");
            }
            
            NodeEvent {
                event: NodeEventKind::UserStateChange { id, muted, deafened },
                ..
            } => {
                if let Some(session) = crate::wt::GLOBAL_SESSIONS.iter().find(|s| s.session_id == id) {
                    let session_id = session.id.clone();
                    let call_id = session.call_id.clone();
                    
                    let mut session_data = session.session_data.write().await;
                    session_data.can_speak = !muted;
                    session_data.can_listen = !deafened;
                    
                    if muted {
                        if let Some(call) = crate::wt::GLOBAL_CALLS.get(&call_id) {
                            for track in session_data.producers.values() {
                                if matches!(track.media_hint, pulse_api::MediaHint::Audio) {
                                    for member_id in call.members.iter() {
                                        if *member_id.key() == session_id {
                                            continue;
                                        }
                                        if let Some(member_session) = crate::wt::GLOBAL_SESSIONS.get(member_id.key()) {
                                            let _ = member_session.message_tx.send(pulse_api::WtMessageS2C::TrackUnavailable {
                                                id: track.id.clone(),
                                            });
                                        }
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
                if let Some((_, session)) = crate::wt::GLOBAL_SESSIONS.iter()
                    .find(|s| s.session_id == id)
                    .map(|s| (s.id.clone(), s.clone())) 
                {
                    let _ = session.message_tx.send(pulse_api::WtMessageS2C::Disconnected {
                        reconnect: None,
                    });
                    
                    session.connection.close(0u32.into(), b"User disconnected by server");
                }
            }
            
            NodeEvent {
                event: NodeEventKind::UserMoved { id, target_server, target_token },
                ..
            } => {
                if let Some((_, session)) = crate::wt::GLOBAL_SESSIONS.iter()
                    .find(|s| s.session_id == id)
                    .map(|s| (s.id.clone(), s.clone()))
                {
                    let _ = session.message_tx.send(pulse_api::WtMessageS2C::Disconnected {
                        reconnect: Some((target_server, target_token)),
                    });
                    
                    session.connection.close(0u32.into(), b"User moved to another server");
                }
            }
            
            NodeEvent {
                event: NodeEventKind::CallEnded { call_id },
                ..
            } => {
                if let Some((_, call)) = crate::wt::GLOBAL_CALLS.remove(&call_id) {
                    for member_id in call.members.iter() {
                        if let Some(session) = crate::wt::GLOBAL_SESSIONS.get(member_id.key()) {
                            let _ = session.message_tx.send(pulse_api::WtMessageS2C::Disconnected {
                                reconnect: None,
                            });
                            
                            session.connection.close(0u32.into(), b"Call ended");
                        }
                    }
                }
            }
            
            _ => {}
        }
    }
    ()
}
