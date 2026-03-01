use std::time::Duration;

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
    RPC_CLIENTS,
    errors::{Error, Result},
    methods::{Event, UserJoinedCallEvent, UserLeftCallEvent, emit_to_ids},
    services::utilities::generate_token,
};

use super::{
    database::calls::Call,
    redis::{INSTANCE_ID, get_connection, get_pubsub},
    utilities::{deserialize, serialize},
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
    pub fn suppress(&self) {
        // TODO:!! disable node and clean up calls (move to other server if possible)
    }

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
        let mut pubsub = get_pubsub().await;
        pubsub.subscribe("nodes").await.unwrap();
        let mut connection = get_connection().await;
        connection
            .publish::<&str, NodeEvent, ()>(
                "nodes",
                NodeEvent {
                    event: NodeEventKind::Query,
                    id: INSTANCE_ID.clone(),
                },
            )
            .await
            .expect("Failed to publish");
        while let Some(msg) = pubsub.on_message().next().await {
            let payload: Vec<u8> = msg.get_payload().unwrap();
            let payload: NodeEvent = deserialize(&payload).unwrap();
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
                    AVAILABLE_NODES.remove(&id);
                    info!("Node {} disconnected", id);
                }
                _ => {}
            }
        }
    });

    // stream for user lifecycle events
    task::spawn(async move {
        let mut connection = get_connection().await;
        let consumer_name = INSTANCE_ID.clone();

        loop {
            let result = connection
                .xread_options::<_, _, redis::streams::StreamReadReply>(
                    &["voice:events:user-lifecycle"],
                    &[">"],
                    &redis::streams::StreamReadOptions::default()
                        .group("harmony-servers", &consumer_name)
                        .block(5000)
                        .count(10),
                )
                .await;

            let reply = match result {
                Ok(reply) => reply,
                Err(e) => {
                    tracing::error!("Failed to read from stream: {:?}", e);
                    time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };

            for stream_key in &reply.keys {
                for stream_id in &stream_key.ids {
                    let message_id = &stream_id.id;

                    let event_data = match stream_id.map.get("data") {
                        Some(redis::Value::BulkString(bytes)) => bytes,
                        _ => {
                            tracing::warn!("Invalid stream message format");
                            continue;
                        }
                    };

                    let payload: NodeEvent = match deserialize(event_data) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!("Failed to deserialize event: {:?}", e);
                            let _ = connection
                                .xack::<_, _, _, ()>(
                                    "voice:events:user-lifecycle",
                                    "harmony-servers",
                                    &[message_id],
                                )
                                .await;
                            continue;
                        }
                    };

                    let process_result = match payload.event {
                        NodeEventKind::UserDisconnect {
                            ref id,
                            ref call_id,
                        } => process_user_disconnect(id, call_id).await,
                        NodeEventKind::UserConnect {
                            ref id,
                            ref call_id,
                        } => process_user_connect(id, call_id).await,
                        _ => Ok(()),
                    };

                    if let Err(e) = process_result {
                        tracing::error!("Failed to process event: {:?}", e);
                        // should be retried later
                        continue;
                    }

                    let _ = connection
                        .xack::<_, _, _, ()>(
                            "voice:events:user-lifecycle",
                            "harmony-servers",
                            &[message_id],
                        )
                        .await;
                }
            }
        }
    });

    // monitor node timeout
    task::spawn(async move {
        loop {
            let time = chrono::Utc::now().timestamp_millis();
            AVAILABLE_NODES.retain(|id, node| {
                if node.last_ping + 10000 < time {
                    node.suppress();
                    info!("Node {} timed out", id);
                    false // Remove node
                } else {
                    true // Keep node
                }
            });
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
                // TODO: handle dead nodes that cause calls to be stuck in memory and never expire
                if let Ok(Some(call)) = ActiveCall::get(&call_id.0).await
                    && let Err(e) = call.end().await
                {
                    tracing::error!("Failed to end call {}: {:?}", call_id.0, e);
                    continue;
                }
            }
        }
    });
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

            let clients = RPC_CLIENTS.get().expect("RPC clients not initialized");
            let member_user_ids: Vec<String> = call
                .members
                .iter()
                .map(|session| session.user_id.clone())
                .collect();

            emit_to_ids(
                clients.clone(),
                &member_user_ids,
                Event::UserLeftCall(UserLeftCallEvent {
                    call_id: call_id.to_string(),
                    session_id: session_id.to_string(),
                }),
            );
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
            let clients = RPC_CLIENTS.get().expect("RPC clients not initialized");
            emit_to_ids(
                clients.clone(),
                &member_ids,
                Event::UserJoinedCall(UserJoinedCallEvent {
                    call_id: call_id.to_string(),
                    user_id: session.user_id.clone(),
                    session_id: session.id.clone(),
                    muted: session.muted,
                    deafened: session.deafened,
                }),
            );
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
                let data = deserialize(bytes);
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
        let data = serialize(self).unwrap();
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

        let member_ids: Vec<String> = self
            .members
            .iter()
            .map(|session| session.user_id.clone())
            .collect();
        Call::update(&self.id, member_ids).await?;

        redis
            .publish::<&str, NodeEvent, ()>(
                "nodes",
                NodeEvent {
                    id: INSTANCE_ID.clone(),
                    event: NodeEventKind::CallEnded {
                        call_id: self.id.clone(),
                    },
                },
            )
            .await?;

        info!("Call {} ended", self.id);

        Ok(())
    }
}
