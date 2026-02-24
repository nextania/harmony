use pulse_types::Region;
use redis::{FromRedisValue, ToRedisArgs, ToSingleRedisArg};
use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionData {
    pub session_id: String,
    pub call_id: String,
    pub assigned_server: String,
    pub can_listen: bool,
    pub can_speak: bool,
    pub can_video: bool,
    pub can_screen: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeDescription {
    pub region: Region,
    pub server_address: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeEvent {
    pub id: String,
    pub event: NodeEventKind,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum NodeEventKind {
    Description(NodeDescription), // when a node becomes available
    Ping,                         // periodic ping from node
    Disconnect,                   // when a node goes offline
    Query,                        // when the main server requests all available nodes
    UserConnect {
        // IMPORTANT: this is the session id, not the user id
        // one user may connect several times to one call
        id: String,
        call_id: String,
    }, // Notify the main server that a user has connected
    UserDisconnect {
        id: String,
        call_id: String,
    }, // Notify the main server that a user has disconnected (or be notified by the main server)
    UserStateChange {
        id: String,
        muted: bool,
        deafened: bool,
    }, // The main server handles state and notifies the node of a user mute/deafen state change
    UserMoved {
        id: String,
        target_server: String,
        target_token: String,
    }, // The main server notifies the node that a user has moved regions
    CallEnded {
        call_id: String,
    }, // The main server notifies the node that a call has ended, disconnecting all users in that call
}

impl ToSingleRedisArg for NodeEvent {}

impl ToRedisArgs for NodeEvent {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let data = serialize(self).unwrap();
        out.write_arg(data.as_slice());
    }
}

impl FromRedisValue for NodeEvent {
    fn from_redis_value(v: redis::Value) -> Result<Self, redis::ParsingError> {
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

impl ToSingleRedisArg for SessionData {}

impl ToRedisArgs for SessionData {
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        let data = serialize(self).unwrap();
        out.write_arg(data.as_slice());
    }
}

impl FromRedisValue for SessionData {
    fn from_redis_value(v: redis::Value) -> Result<Self, redis::ParsingError> {
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

pub fn serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut buf = Vec::new();
    value.serialize(&mut Serializer::new(&mut buf).with_struct_map())?;
    Ok(buf)
}

pub fn deserialize<T: for<'a> Deserialize<'a>>(buf: &[u8]) -> Result<T, rmp_serde::decode::Error> {
    let mut deserializer = Deserializer::new(buf);
    Deserialize::deserialize(&mut deserializer)
}
