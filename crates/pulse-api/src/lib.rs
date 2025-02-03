

use std::str::FromStr;

use redis::FromRedisValue;

use redis::ToRedisArgs;
use rmp_serde::Deserializer;
use rmp_serde::Serializer;
use serde::Deserialize;
use serde::Serialize;

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
    Description(NodeDescription), // on node connect
    Ping, // on node ping
    Disconnect, // on node disconnect
    Query, // on server connect
    UserConnect {
        session_id: String,
        call_id: String,
        sdp: SessionDescription
    }, // server -> node on user connect
    UserCreate {
        session_id: String,
        call_id: String,
        sdp: SessionDescription,
    }, // node -> server on new user
    StartProduce {
        track: String
    }, // server -> node on user start produce
    StopProduce {
        track: String

    },// server -> node on user start produce
    StartConsume {
        track: String
    }, // server -> node on user start consume
    StopConsume {
        track: String
    }, // server -> node on user stop consume
    UserDisconnect {
        id: String
    }, // server -> node on user disconnect
    UserDelete {
        id: String
    }, // node -> server on user delete
    TrackAvailable {
        id: String
    },
    TrackUnavailable {
        id: String
    },
}

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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
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


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "sdp")]
pub enum SessionDescription {
    #[serde(rename = "offer")]
    Offer(String),
    #[serde(rename = "answer")]
    Answer(String),
}

impl ToString for SessionDescription {
    fn to_string(&self) -> String {
        match self {
            SessionDescription::Offer(sdp) => sdp.clone(),
            SessionDescription::Answer(sdp) => sdp.clone(),
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
