use std::time::Duration;

use async_nats::jetstream::consumer::{AckPolicy, pull};
use common::nats::{STREAM_VOICE_LIFECYCLE, SUBJECT_NODES_ALL, subject_node};
use common::{NodeDescription, NodeEvent, NodeEventKind, SessionData};
use dashmap::DashMap;
use futures_util::StreamExt;
use lazy_static::lazy_static;
use pulse_types::Region;
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs, ToSingleRedisArg};
use serde::{Deserialize, Serialize};
use tokio::{task, time};
use tracing::info;

use crate::{
    errors::{Error, Result},
    methods::{CallMigratedEvent, Event, UserJoinedCallEvent, UserLeftCallEvent},
    services::{events, nats, utilities::generate_token},
};

use super::{
    database::calls::Call,
    redis::{INSTANCE_ID, get_connection},
};

lazy_static! {
    pub static ref AVAILABLE_NODES: DashMap<String, Node> = DashMap::new();
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: String,
    pub region: Region,
    pub server_address: String,
    pub last_ping: i64,
}

impl Node {
    pub fn new(id: String, description: NodeDescription) -> Self {
        let time = chrono::Utc::now().timestamp_millis();
        Node {
            id,
            region: description.region,
            server_address: description.server_address,
            last_ping: time,
        }
    }
}

pub fn spawn_voice_events() {
    // node events
    task::spawn(async move {
        let client = nats::client();
        let mut sub = match client.subscribe(SUBJECT_NODES_ALL).await {
            Ok(sub) => sub,
            Err(e) => {
                tracing::error!("Failed to subscribe to {}: {:?}", SUBJECT_NODES_ALL, e);
                return;
            }
        };
        // request all available nodes to announce themselves
        nats::publish_node_event(
            SUBJECT_NODES_ALL.to_string(),
            &NodeEvent {
                event: NodeEventKind::Query,
                id: INSTANCE_ID.clone(),
            },
        )
        .await;

        while let Some(msg) = sub.next().await {
            let payload: NodeEvent = match serde_cbor_2::from_slice(&msg.payload) {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!("Failed to deserialize node event: {:?}", e);
                    continue;
                }
            };
            match payload {
                NodeEvent {
                    id,
                    event: NodeEventKind::Description(description),
                    ..
                } => {
                    let node: Node = Node::new(id, description);
                    if AVAILABLE_NODES.contains_key(&node.id) {
                        continue;
                    }
                    let i = node.id.clone();
                    AVAILABLE_NODES.insert(node.id.clone(), node);
                    info!("Node {} connected", i);
                }
                NodeEvent {
                    id,
                    event: NodeEventKind::Ping,
                } => {
                    let node = AVAILABLE_NODES.get_mut(&id);
                    if let Some(mut node) = node {
                        node.last_ping = chrono::Utc::now().timestamp_millis();
                    }
                }
                NodeEvent {
                    id,
                    event: NodeEventKind::Disconnect,
                } => {
                    let region = AVAILABLE_NODES.get(&id).map(|n| n.region);
                    AVAILABLE_NODES.remove(&id);
                    info!("Node {} disconnected", id);
                    task::spawn(handle_node_down(id, region));
                }
                _ => {}
            }
        }
    });

    // user lifecycle events
    task::spawn(async move {
        loop {
            if let Err(e) = run_lifecycle_worker().await {
                tracing::error!("Lifecycle worker error: {:?}; recreating consumer in 1s", e);
                time::sleep(Duration::from_secs(1)).await;
            }
        }
    });

    // monitor node timeout
    task::spawn(async move {
        loop {
            let time = chrono::Utc::now().timestamp_millis();
            let mut dead_nodes: Vec<(String, Region)> = Vec::new();
            AVAILABLE_NODES.retain(|id, node| {
                if node.last_ping + 10000 < time {
                    info!("Node {} timed out", id);
                    dead_nodes.push((id.clone(), node.region));
                    false // Remove node
                } else {
                    true // Keep node
                }
            });
            for (id, region) in dead_nodes {
                task::spawn(handle_node_down(id, Some(region)));
            }
            // Don't deadlock
            time::sleep(std::time::Duration::from_millis(1000)).await;
        }
    });

    // move expired calls to database
    task::spawn(async move {
        let mut redis = get_connection().await;
        loop {
            let time = chrono::Utc::now().timestamp_millis() - 30000;
            let expired_calls: Vec<(String, i64)> = redis
                .bzpopmin("voice:empty-calls", 1.0)
                .await
                .unwrap_or_default();
            let call_id = expired_calls.first();
            if let Some(call_id) = call_id {
                // if it's not expired, re-add and wait until expired
                if call_id.1 > time {
                    let _ = redis
                        .zadd::<_, _, _, ()>("voice:empty-calls", &call_id.0, call_id.1)
                        .await;
                    time::sleep(Duration::from_millis((call_id.1 - time) as u64)).await;
                    continue;
                }
                if let Ok(Some(call)) = ActiveCall::get(&call_id.0).await
                    && let Err(e) = call.end().await
                {
                    tracing::error!("Failed to end call {}: {:?}", call_id.0, e);
                    // don't leak in Redis
                    let _ = redis
                        .zadd::<_, _, _, ()>("voice:empty-calls", &call_id.0, call_id.1)
                        .await;
                    time::sleep(Duration::from_millis(1000)).await;
                    continue;
                }
            }
        }
    });
}

async fn run_lifecycle_worker() -> std::result::Result<(), async_nats::Error> {
    let js = nats::jetstream();
    let stream = js.get_stream(STREAM_VOICE_LIFECYCLE).await?;
    let consumer = stream
        .create_consumer(pull::Config {
            durable_name: Some("lifecycle-worker".to_string()),
            name: Some("lifecycle-worker".to_string()),
            ack_policy: AckPolicy::Explicit,
            ack_wait: Duration::from_secs(30),
            max_deliver: 5,
            filter_subject: "voice.lifecycle.>".to_string(),
            ..Default::default()
        })
        .await?;

    let mut messages = consumer.messages().await?;
    while let Some(message) = messages.next().await {
        let message = message?;
        let payload: NodeEvent = match serde_cbor_2::from_slice(&message.payload) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to deserialize lifecycle event: {:?}", e);
                message.ack().await.ok();
                continue;
            }
        };

        let process_result = match payload.event {
            NodeEventKind::UserConnect {
                ref id,
                ref call_id,
            } => process_user_connect(id, call_id).await,
            NodeEventKind::UserDisconnect {
                ref id,
                ref call_id,
            } => process_user_disconnect(id, call_id).await,
            _ => Ok(()),
        };

        match process_result {
            Ok(()) => {
                let _ = message.ack().await;
            }
            Err(e) => {
                tracing::error!("Failed to process lifecycle event: {:?}", e);
            }
        }
    }
    Ok(())
}

async fn process_user_disconnect(session_id: &str, call_id: &str) -> Result<()> {
    if let Ok(Some(mut call)) = ActiveCall::get(&call_id.to_string()).await {
        if let Err(e) = call.leave_user(&session_id.to_string()).await {
            tracing::error!(
                "Failed to remove user {} from call {}: {:?}",
                session_id,
                call_id,
                e
            );
            return Err(e);
        } else {
            info!("User {} disconnected from call {}", session_id, call_id);

            let member_user_ids: Vec<String> = call
                .members
                .iter()
                .map(|session| session.user_id.clone())
                .collect();

            events::publish(
                &member_user_ids,
                Event::UserLeftCall(UserLeftCallEvent {
                    call_id: call_id.to_string(),
                    session_id: session_id.to_string(),
                }),
            )
            .await;
        }
    }
    Ok(())
}

async fn process_user_connect(session_id: &str, call_id: &str) -> Result<()> {
    info!("User {} connected to call {}", session_id, call_id);
    if let Ok(call) = ActiveCall::get(&call_id.to_string()).await {
        if let Some(mut call) = call {
            call.empty_since = None;
            // remove item from pending sessions
            let session: Vec<CallSession> = call
                .pending_sessions
                .extract_if(.., |s| s.id == session_id)
                .collect();
            let Some(session) = session.first() else {
                tracing::warn!(
                    "Session {} not found in pending sessions for call {}",
                    session_id,
                    call_id
                );
                return Err(Error::NotFound);
            };
            call.members.push(session.clone());
            if let Err(e) = call.update().await {
                tracing::error!("Failed to update call {} in redis: {:?}", call_id, e);
                return Err(e);
            }
            let member_ids: Vec<String> = call
                .members
                .iter()
                .map(|session| session.user_id.clone())
                .collect();
            // including the one who just joined
            events::publish(
                &member_ids,
                Event::UserJoinedCall(UserJoinedCallEvent {
                    call_id: call_id.to_string(),
                    user_id: session.user_id.clone(),
                    session_id: session.id.clone(),
                    muted: session.muted,
                    deafened: session.deafened,
                }),
            )
            .await;
        } else {
            // TODO:!! this is most likely due to the call expiring while user is connecting
            // this should RESTORE the call into memory
            tracing::warn!(
                "Call {} not found when user {} tried to connect",
                call_id,
                session_id
            );
        }
    }
    Ok(())
}

pub async fn handle_node_down(node_id: String, region: Option<Region>) {
    let mut redis = get_connection().await;
    let index_key = format!("node:{}:calls", node_id);
    let call_ids: Vec<String> = redis.smembers(&index_key).await.unwrap_or_default();
    if call_ids.is_empty() {
        let _: std::result::Result<(), _> = redis.del::<_, ()>(&index_key).await;
        return;
    }
    info!(
        "Node {} down: recovering {} call(s)",
        node_id,
        call_ids.len()
    );

    for call_id in call_ids {
        let Ok(Some(mut call)) = ActiveCall::get(&call_id).await else {
            let _: std::result::Result<(), _> = redis.srem::<_, _, ()>(&index_key, &call_id).await;
            continue;
        };

        let affected_users: Vec<String> = call
            .members
            .iter()
            .chain(call.pending_sessions.iter())
            .map(|s| s.user_id.clone())
            .collect();
        let member_sessions: Vec<(String, String)> = call
            .members
            .iter()
            .map(|s| (s.user_id.clone(), s.id.clone()))
            .collect();

        // pick an alternative node to move users
        let target = region
            .as_ref()
            .and_then(|r| {
                AVAILABLE_NODES
                    .iter()
                    .find(|n| n.region == *r && n.id != node_id)
                    .map(|n| (n.id.clone(), n.server_address.clone()))
            })
            .or_else(|| {
                AVAILABLE_NODES
                    .iter()
                    .find(|n| n.id != node_id)
                    .map(|n| (n.id.clone(), n.server_address.clone()))
            });

        match target {
            Some((target_id, target_addr)) => {
                if let Err(e) = call.migrate_to(&node_id, &target_id, &target_addr).await {
                    tracing::error!(
                        "Failed to migrate call {} to node {}: {:?}; ending it instead",
                        call_id,
                        target_id,
                        e
                    );
                    if let Err(e) = call.end().await {
                        tracing::error!("Failed to end call {}: {:?}", call_id, e);
                        let _: std::result::Result<(), _> =
                            redis.srem::<_, _, ()>(&index_key, &call_id).await;
                    }
                    emit_call_ended(&call_id, &affected_users, &member_sessions).await;
                    continue;
                }
                info!(
                    "Migrated call {} from node {} to node {}",
                    call_id, node_id, target_id
                );
                events::publish(
                    &affected_users,
                    Event::CallMigrated(CallMigratedEvent {
                        call_id: call_id.clone(),
                        server_address: target_addr.clone(),
                    }),
                )
                .await;
            }
            None => {
                info!("No alternative node for call {}; ending it", call_id);
                if let Err(e) = call.end().await {
                    tracing::error!("Failed to end call {}: {:?}", call_id, e);
                    let _: std::result::Result<(), _> =
                        redis.srem::<_, _, ()>(&index_key, &call_id).await;
                }
                emit_call_ended(&call_id, &affected_users, &member_sessions).await;
            }
        }
    }

    let _: std::result::Result<(), _> = redis.del::<_, ()>(&index_key).await;
}

async fn emit_call_ended(
    call_id: &str,
    affected_users: &[String],
    member_sessions: &[(String, String)],
) {
    for (_user_id, session_id) in member_sessions {
        events::publish(
            affected_users,
            Event::UserLeftCall(UserLeftCallEvent {
                call_id: call_id.to_string(),
                session_id: session_id.clone(),
            }),
        )
        .await;
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActiveCall {
    pub id: String,
    pub name: Option<String>,
    pub members: Vec<CallSession>,
    pub channel_id: String,
    pub assigned_node: String,
    pub server_address: String,
    pub empty_since: Option<i64>,
    pub pending_sessions: Vec<CallSession>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CallSession {
    pub id: String,
    pub user_id: String,
    pub call_id: String,
    pub muted: bool,
    pub deafened: bool,
}

impl FromRedisValue for ActiveCall {
    fn from_redis_value(v: redis::Value) -> std::result::Result<Self, redis::ParsingError> {
        match v {
            redis::Value::BulkString(ref bytes) => {
                let data = serde_cbor_2::from_slice(bytes);
                match data {
                    Ok(data) => Ok(data),
                    Err(_) => Err(redis::ParsingError::from("Deserialization error")),
                }
            }

            _ => Err(redis::ParsingError::from("Format error")),
        }
    }
}

impl ToSingleRedisArg for ActiveCall {}

impl ToRedisArgs for ActiveCall {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let data = serde_cbor_2::to_vec(self).unwrap();
        out.write_arg(data.as_slice());
    }
}

impl ActiveCall {
    pub async fn create(
        channel: &String,
        initiator: &str,
        preferred_region: Option<Region>,
    ) -> Result<ActiveCall> {
        let mut redis = get_connection().await;
        let call = Self::get_in_channel(channel).await?;
        if call.is_some() {
            return Err(Error::AlreadyExists);
        }
        // assign node
        let (assigned_node, server_address) = if let Some(region) = preferred_region
            && let Some(node) = AVAILABLE_NODES.iter().find(|n| n.region == region)
        {
            (node.id.clone(), node.server_address.clone())
        } else {
            // fallback to any node
            let node = AVAILABLE_NODES.iter().next();
            if let Some(node) = node {
                (node.id.clone(), node.server_address.clone())
            } else {
                return Err(Error::NoVoiceNodesAvailable);
            }
        };
        let time = chrono::Utc::now().timestamp_millis();
        let call = ActiveCall {
            id: ulid::Ulid::new().to_string(),
            name: None,
            members: vec![],
            channel_id: channel.clone(),
            assigned_node,
            server_address,
            empty_since: Some(time),
            pending_sessions: vec![],
        };
        redis
            .set::<String, ActiveCall, ()>(format!("call:{}", call.id), call.clone())
            .await
            .unwrap();
        redis
            .set::<String, String, ()>(format!("call:channel:{}", channel), call.id.clone())
            .await
            .unwrap();
        redis
            .zadd::<_, _, _, ()>("voice:empty-calls", &call.id, time)
            .await?;
        redis
            .sadd::<_, _, ()>(format!("node:{}:calls", call.assigned_node), &call.id)
            .await?;
        let stored_call = Call {
            channel_id: channel.clone(),
            id: call.id.clone(),
            joined_members: vec![],
            name: None,
            ended_at: time,
            initiator: initiator.to_owned(),
        };
        stored_call.create().await?;
        Ok(call)
    }

    pub async fn get_in_channel(channel: &String) -> Result<Option<ActiveCall>> {
        let mut redis = get_connection().await;
        let id: Option<String> = redis.get(format!("call:channel:{}", channel)).await?;
        if let Some(id) = id {
            Ok(Self::get(&id).await?)
        } else {
            Ok(None)
        }
    }

    pub async fn get(id: &String) -> Result<Option<ActiveCall>> {
        let mut redis = get_connection().await;
        let call: Option<ActiveCall> = redis.get(format!("call:{}", id)).await?;
        Ok(call)
    }

    pub async fn update(&self) -> Result<()> {
        let mut redis = get_connection().await;
        redis
            .set::<String, ActiveCall, ()>(format!("call:{}", self.id), self.clone())
            .await?;
        if !self.members.is_empty() {
            let _: () = redis.zrem("voice:empty-calls", &self.id).await?;
        }

        let member_ids: Vec<String> = self
            .members
            .iter()
            .map(|session| session.user_id.clone())
            .collect();
        if let Err(e) = Call::update(&self.id.to_string(), member_ids.clone()).await {
            tracing::error!("Failed to update call {} in database: {:?}", self.id, e);
            return Err(e);
        }
        Ok(())
    }

    pub async fn create_token(
        &mut self,
        user_id: &str,
        initial_muted: bool,
        initial_deafened: bool,
    ) -> Result<(String, String)> {
        let session_id = ulid::Ulid::new().to_string();
        self.pending_sessions.push(CallSession {
            id: session_id.clone(),
            user_id: user_id.to_string(),
            call_id: self.id.clone(),
            muted: initial_muted,
            deafened: initial_deafened,
        });
        self.update().await?;

        let token = generate_token();
        let mut redis = get_connection().await;
        redis
            .set_ex::<String, SessionData, ()>(
                format!("session:{}", token),
                SessionData {
                    call_id: self.id.clone(),
                    session_id: session_id.clone(),
                    assigned_server: self.assigned_node.clone(),

                    can_listen: !initial_deafened,
                    can_speak: !initial_muted,
                    // TODO:!! support limiting video/screen
                    can_screen: true,
                    can_video: true,
                },
                60,
            )
            .await?;
        Ok((session_id, token))
    }

    pub async fn migrate_to(
        &mut self,
        old_node: &str,
        node_id: &str,
        server_address: &str,
    ) -> Result<()> {
        let mut redis = get_connection().await;
        self.assigned_node = node_id.to_string();
        self.server_address = server_address.to_string();
        self.members.clear();
        self.pending_sessions.clear();
        let time = chrono::Utc::now().timestamp_millis();
        self.empty_since = Some(time);

        redis
            .set::<String, ActiveCall, ()>(format!("call:{}", self.id), self.clone())
            .await?;
        redis
            .zadd::<_, _, _, ()>("voice:empty-calls", &self.id, time)
            .await?;
        redis
            .srem::<_, _, ()>(format!("node:{}:calls", old_node), &self.id)
            .await?;
        redis
            .sadd::<_, _, ()>(format!("node:{}:calls", node_id), &self.id)
            .await?;
        Ok(())
    }

    pub async fn leave_user(&mut self, session_id: &String) -> Result<()> {
        self.members.retain(|x| x.id != *session_id);
        if self.members.is_empty() {
            let time = chrono::Utc::now().timestamp_millis();
            self.empty_since = Some(time);
            let mut redis = get_connection().await;
            redis
                .zadd::<_, _, _, ()>("voice:empty-calls", &self.id, time)
                .await?;
        }
        self.update().await?;

        Ok(())
    }

    pub async fn end(&self) -> Result<()> {
        let mut redis = get_connection().await;

        redis
            .del::<std::string::String, ()>(format!("call:channel:{}", self.channel_id))
            .await?;
        redis
            .del::<std::string::String, ()>(format!("call:{}", self.id))
            .await?;
        redis
            .zrem::<_, _, ()>("voice:empty-calls", &self.id)
            .await?;
        redis
            .srem::<_, _, ()>(format!("node:{}:calls", self.assigned_node), &self.id)
            .await?;

        let member_ids: Vec<String> = self
            .members
            .iter()
            .map(|session| session.user_id.clone())
            .collect();
        Call::update(&self.id, member_ids).await?;

        nats::publish_node_event(
            subject_node(&self.assigned_node),
            &NodeEvent {
                id: INSTANCE_ID.clone(),
                event: NodeEventKind::CallEnded {
                    call_id: self.id.clone(),
                },
            },
        )
        .await;

        info!("Call {} ended", self.id);

        Ok(())
    }
}
