use std::time::Duration;

use dashmap::DashMap;
use futures_util::StreamExt;
use lazy_static::lazy_static;
use log::info;
use pulse_api::{NodeDescription, NodeEvent, NodeEventKind, Region, SessionData};
use redis::{AsyncCommands, FromRedisValue, ToRedisArgs};
use serde::{Deserialize, Serialize};
use tokio::{task, time};

use crate::{
    errors::{Error, Result},
    request::Request, services::encryption::generate_token,
};

use super::{
    database::calls::Call,
    encryption::{deserialize, serialize},
    redis::{get_connection, get_pubsub},
};

lazy_static! {
    pub static ref AVAILABLE_NODES: DashMap<String, Node> = DashMap::new();
    pub static ref REQUESTS: DashMap<String, Request<String>> = DashMap::new();
}

#[derive(Clone, Debug)]
pub struct Node {
    id: String,
    region: Region,
    last_ping: i64,
}

impl Node {
    pub fn suppress(&self) {
        // TODO: disable node and clean up calls (move to other server if possible)
    }

    pub fn new(id: String, description: NodeDescription) -> Self {
        let time = chrono::Utc::now().timestamp_millis();
        Node {
            id,
            region: description.region,
            last_ping: time,
        }
    }
}

pub fn spawn_voice_events() {
    task::spawn(async move {
        let mut pubsub = get_pubsub().await;
        pubsub.subscribe("nodes").await.unwrap();
        let mut connection = get_connection().await;
        connection
            .publish::<&str, NodeEvent, ()>(
                "nodes",
                NodeEvent {
                    event: NodeEventKind::Query,
                    id: "server".to_owned(),
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
                NodeEvent {
                    event: NodeEventKind::UserDisconnect { id: session_id, call_id },
                    ..
                } => {
                    if let Ok(Some(mut call)) = ActiveCall::get(&call_id).await {
                        if let Err(e) = call.leave_user(&session_id).await {
                            log::error!("Failed to remove user {} from call {}: {:?}", session_id, call_id, e);
                        } else {
                            info!("User {} disconnected from call {}", session_id, call_id);
                        }
                    }
                }
                NodeEvent {
                    event: NodeEventKind::UserConnect { id: session_id, call_id },
                    ..
                } => {
                    info!("User {} connected to call {}", session_id, call_id);
                    if let Ok(Some(mut call)) = ActiveCall::get(&call_id).await {
                        call.empty_since = None;
                        if let Err(e) = call.update().await {
                            log::error!("Failed to update call {} in redis: {:?}", call_id, e);
                        }
                        let member_ids: Vec<String> = call.members.iter().map(|(user_id, _)| user_id.clone()).collect();
                        if let Err(e) = Call::update(&call_id, member_ids).await {
                            log::error!("Failed to update call {} in database: {:?}", call_id, e);
                        }
                    }
                }
                NodeEvent { .. } => {}
            }
        }
    });
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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActiveCall {
    pub id: String,
    pub name: Option<String>,
    pub members: Vec<(String, String)>, // (user_id, session_id)
    pub channel_id: String,
    pub assigned_node: String,
    pub empty_since: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CallSession {
    id: String,
    user_id: String,
    call_id: String,
    muted: bool,
    deafened: bool,
    speaking: bool,
    video: bool,
    screenshare: bool,
}

impl FromRedisValue for ActiveCall {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
        match *v {
            redis::Value::BulkString(ref bytes) => {
                let data = deserialize(bytes);
                match data {
                    Ok(data) => Ok(data),
                    Err(_) => Err(redis::RedisError::from((
                        redis::ErrorKind::TypeError,
                        "Deserialization error",
                    ))),
                }
            }

            _ => Err(redis::RedisError::from((
                redis::ErrorKind::TypeError,
                "Format error",
            ))),
        }
    }
}

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
    pub async fn create(channel: &String, initiator: &str, preferred_region: Option<Region>) -> Result<ActiveCall> {
        let mut redis = get_connection().await;
        let call = Self::get_in_channel(channel).await?;
        if call.is_some() {
            return Err(Error::AlreadyExists);
        }
        // assign node
        let assigned_node = if let Some(region) = preferred_region &&let Some(node) = AVAILABLE_NODES
                .iter()
                .find(|n| n.region == region)
                .map(|n| n.id.clone()) {
                    
                node
            } else {
                // fallback to any node
                let node = AVAILABLE_NODES.iter().next().map(|n| n.id.clone());
                if let Some(node) = node {
                    node
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
            empty_since: Some(time),
        };
        redis
            .set::<std::string::String, ActiveCall, ()>(
                format!("call:channel:{}", channel),
                call.clone(),
            )
            .await
            .unwrap();
        let stored_call = Call {
            channel_id: channel.clone(),
            id: call.id.clone(),
            joined_members: vec![],
            name: None,
            ended_at: time,
            initiator: initiator.to_owned(),
        };
        stored_call.create().await?;
        let call_id = call.id.clone();
        
        task::spawn(async move {
            const EMPTY_TIMEOUT_MS: i64 = 5 * 60 * 1000; // 5 minutes
            let mut last_empty = time;
            
            loop {
                // sleep until last_empty + EMPTY_TIMEOUT_MS
                time::sleep(Duration::from_millis(
                    (last_empty + EMPTY_TIMEOUT_MS - chrono::Utc::now().timestamp_millis()) as u64,
                )).await;
                
                let call = match ActiveCall::get(&call_id).await {
                    Ok(Some(call)) => call,
                    Ok(None) => {
                        info!("Call {} no longer exists, stopping monitor", call_id);
                        break;
                    }
                    Err(e) => {
                        log::error!("Failed to get call {}: {:?}", call_id, e);
                        continue;
                    }
                };
                
                if call.members.is_empty() 
                && let Some(empty_time) = call.empty_since {
                    let now = chrono::Utc::now().timestamp_millis();
                    if now - empty_time >= EMPTY_TIMEOUT_MS {
                        info!("Call {} has been empty for 5 minutes, ending call", call_id);
                        if let Err(e) = call.end().await {
                            log::error!("Failed to end call {}: {:?}", call_id, e);
                        }
                        break;
                    }
                    last_empty = empty_time;
                    
                }
            }
        });

        task::spawn(async move {
            loop {
                // TODO: periodically update ended_at in db
            }
        });
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
            .set::<String, ActiveCall, ActiveCall>(format!("call:{}", self.id), self.clone())
            .await?;
        Ok(())
    }
    
    pub async fn get_token(&mut self, user_id: &String) -> Result<String> {
        let session_id = ulid::Ulid::new().to_string();
        self.members.push((user_id.clone(), session_id.clone()));
        self.update().await?;
        
        let member_ids: Vec<String> = self.members.iter().map(|(uid, _)| uid.clone()).collect();
        Call::update(&self.id, member_ids).await?;
        
        let token = generate_token();
        let mut redis = get_connection().await;
        redis
            .set::<String, SessionData, ()>(
                format!("session:{}", token),
                SessionData {
                    call_id: self.id.clone(),
                    session_id: session_id.clone(),
                    assigned_server: self.assigned_node.clone(),

                    // TODO: set based on 1) permissions and 2) user requested settings
                    can_listen: true,
                    can_speak: true,
                    can_screen: true,
                    can_video: true,
                },
            )
            .await?;
        Ok(token)
    }

    pub async fn leave_user(&mut self, session_id: &String) -> Result<()> {
        self.members.retain(|x| x.1 != *session_id);
        if self.members.is_empty() {
            let time = chrono::Utc::now().timestamp_millis();
            self.empty_since = Some(time);
        }
        self.update().await?;
        
        let member_ids: Vec<String> = self.members.iter().map(|(user_id, _)| user_id.clone()).collect();
        Call::update(&self.id, member_ids).await?;
        
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
        
        let member_ids: Vec<String> = self.members.iter().map(|(user_id, _)| user_id.clone()).collect();
        Call::update(&self.id, member_ids).await?;
        
        redis
            .publish::<&str, NodeEvent, ()>(
                "nodes",
                NodeEvent {
                    id: "server".to_owned(),
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
