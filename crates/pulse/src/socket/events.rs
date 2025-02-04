use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum MediaType {
    Audio,
    Video,
    ScreenAudio,
    ScreenVideo,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteTrack {
    pub id: String,
    pub user_id: String,
    pub media_type: MediaType,
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct IceCandidate {
//     pub candidate: String,
//     #[serde(default)]
//     pub sdp_mid: String,
//     #[serde(default)]
//     pub sdp_mline_index: u16,
//     #[serde(default)]
//     pub username_fragment: String,
// }

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct Negotiation {
//     pub description: SessionDescription,
// }


// impl std::fmt::Display for MediaType {
//     fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
//         match self {
//             MediaType::Audio => write!(f, "Audio"),
//             MediaType::Video => write!(f, "Video"),
//             MediaType::ScreenAudio => write!(f, "ScreenAudio"),
//             MediaType::ScreenVideo => write!(f, "ScreenVideo"),
//         }
//     }
// }
pub fn serialize<T: Serialize>(
    value: &T,
) -> std::result::Result<Vec<u8>, rmp_serde::encode::Error> {
    let mut buf = Vec::new();
    value.serialize(&mut Serializer::new(&mut buf).with_struct_map())?;
    Ok(buf)
}

pub fn deserialize<T: for<'a> Deserialize<'a>>(
    buf: &[u8],
) -> std::result::Result<T, rmp_serde::decode::Error> {
    let mut deserializer = Deserializer::new(buf);
    Deserialize::deserialize(&mut deserializer)
}
