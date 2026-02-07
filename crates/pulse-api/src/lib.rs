use std::str::FromStr;

use redis::FromRedisValue;

use redis::ToRedisArgs;
use redis::ToSingleRedisArg;
use rkyv::Archive;
use rmp_serde::Deserializer;
use rmp_serde::Serializer;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NodeEvent {
    pub id: String,
    #[serde(flatten)]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub enum Region {
    Canada,
    UsCentral,
    UsEast,
    UsWest,
    Europe,
    Asia,
    SouthAmerica,
    Australia,
    Africa,
}

impl FromStr for Region {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "canada" => Ok(Region::Canada),
            "us-central" => Ok(Region::UsCentral),
            "us-east" => Ok(Region::UsEast),
            "us-west" => Ok(Region::UsWest),
            "europe" => Ok(Region::Europe),
            "asia" => Ok(Region::Asia),
            "south-america" => Ok(Region::SouthAmerica),
            "australia" => Ok(Region::Australia),
            "africa" => Ok(Region::Africa),
            _ => Err(()),
        }
    }
}

#[derive(Archive, Clone, Debug, rkyv::Deserialize, rkyv::Serialize)]
pub enum MediaHint {
    Audio,
    Video,
    ScreenAudio,
    ScreenVideo,
}

#[derive(Archive, Clone, Debug, rkyv::Deserialize, rkyv::Serialize)]
pub enum WtMessageC2S {
    Connect {
        session_token: String,
        key_package: Vec<u8>, // Serialized MLS KeyPackage
    },
    Disconnect {},
    StartProduce {
        id: String,
        media_hint: MediaHint,
    },
    StopProduce {
        id: String,
    },
    StartConsume {
        id: String,
    },
    StopConsume {
        id: String,
    },
    Heartbeat {},
    // MLS coordination messages
    MlsCommit {
        commit_data: Vec<u8>,
        epoch: u64,
        welcome_data: Option<Vec<u8>>,
    },
    CommitAck {
        epoch: u64,
    },
}

#[derive(Archive, Clone, Debug, rkyv::Deserialize, rkyv::Serialize)]
pub struct AvailableTrack {
    pub id: String,
    pub media_hint: MediaHint,
    // indicates which session (and therefore user) this track belongs to
    pub session_id: String,
}

#[derive(Archive, Clone, Debug, rkyv::Deserialize, rkyv::Serialize)]
pub enum WtMessageS2C {
    Connected {
        id: String,
        available_tracks: Vec<AvailableTrack>,
    },
    Disconnected {
        reconnect: Option<(String, String)>, // (new_server_address, new_token)
    },
    ProduceStarted {
        id: String,
    },
    ProduceStopped {
        id: String,
    },
    ConsumeStarted {
        id: String,
    },
    ConsumeStopped {
        id: String,
    },
    TrackAvailable {
        track: AvailableTrack,
    },
    TrackUnavailable {
        id: String,
    },
    Heartbeat {},
    // MLS coordination messages
    MlsProposals {
        proposals: Vec<Vec<u8>>,
    },
    MlsCommit {
        epoch: u64, // new epoch
        commit_data: Vec<u8>,
        welcome_data: Option<Vec<u8>>,
    },
    EpochReady {
        epoch: u64, // New epoch number that all members have reached
    },
    // First member should initialize group - server provides external sender credential
    InitializeGroup {
        external_sender_credential: Vec<u8>, // Serialized BasicCredential
        external_sender_signature_key: Vec<u8>, // Public signature key
    },
}

#[derive(Archive, Clone, Debug, rkyv::Deserialize, rkyv::Serialize)]
pub struct WtTrackData {
    // the track id
    pub id: String,
    pub data: Vec<u8>,
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
