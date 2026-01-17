use std::time::Duration;

use dashmap::DashMap;
use futures::{SinkExt, StreamExt, channel::mpsc::unbounded};
use lazy_static::lazy_static;
use once_cell::sync::OnceCell;
use pulse_api::{NodeDescription, NodeEvent, NodeEventKind, SessionDescription};
use redis::{AsyncCommands, Client, aio::MultiplexedConnection};
use str0m::change::SdpOffer;
use tokio::{task, time};
use ulid::Ulid;

use crate::{
    environment::{REDIS_URI, REGION},
    rtc::peer::{ClientApi, ClientApiIn},
    socket::server::{UserCapabilities, UserInformation, create_new_user},
};

static REDIS: OnceCell<Client> = OnceCell::new();

pub async fn connect() {
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

lazy_static! {
    pub static ref CLIENTS: DashMap<String, ClientApi> = DashMap::new();
}

pub async fn listen() -> () {
    let mut pubsub = get_pubsub().await;
    pubsub
        .subscribe("nodes")
        .await
        .expect("Failed to subscribe");
    let instance_id = Ulid::new().to_string();
    let mut connection = get_connection().await;
    connection
        .publish::<&str, NodeEvent, NodeEvent>(
            "nodes",
            NodeEvent {
                event: NodeEventKind::Description(NodeDescription { region: *REGION }),
                id: instance_id.clone(),
            },
        )
        .await;
    let mut c = connection.clone();
    let i = instance_id.clone();
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
        if payload.id == instance_id {
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
                            id: instance_id.clone(),
                        },
                    )
                    .await
                    .expect("Failed to publish");
            }
            NodeEvent {
                event:
                    NodeEventKind::UserConnect {
                        session_id,
                        call_id,
                        sdp,
                    },
                ..
            } => {
                let (send, recv) = unbounded::<NodeEvent>();
                let user = create_new_user(
                    UserInformation {
                        id: session_id.clone(),
                        capabilities: UserCapabilities {
                            audio: true,
                            video: true,
                            screenshare: true,
                        },
                    },
                    call_id,
                    recv,
                )
                .await;
                if let Ok(mut user) = user {
                    let SessionDescription::Offer(offer) = sdp else {
                        continue;
                    };
                    user.send
                        .send(ClientApiIn::Offer(
                            SdpOffer::from_sdp_string(&offer).unwrap(),
                        ))
                        .await
                        .expect("Failed to send offer");
                    CLIENTS.insert(session_id.clone(), user);
                }
            }
            _ => {}
        }
    }
    ()
}
