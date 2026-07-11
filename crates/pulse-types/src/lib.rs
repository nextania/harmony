use std::str::FromStr;

use rkyv::Archive;
use serde::Deserialize;
use serde::Serialize;

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
    Join {
        key_package: Vec<u8>, // Serialized MLS KeyPackage
    },
    StartProduce {
        id: String,
        media_hint: MediaHint,
    },
    StopProduce {
        id: String,
    },
    // MLS coordination messages
    MlsCommit {
        commit_data: Vec<u8>,
        epoch: u64,
        welcome_data: Option<Vec<u8>>,
    },
    CommitAck {
        epoch: u64,
    },
    // Feedback messages
    RequestKeyFrame {
        track_id: String,
    },
    ReceiverReport {
        track_id: String,
        lost: u32,
        received: u32,
        jitter_ms: u32,
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
    TrackAvailable {
        track: AvailableTrack,
    },
    TrackUnavailable {
        id: String,
    },
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
    KeyFrameRequested {
        track_id: String,
    },
    ReceiverReport {
        track_id: String,
        lost: u32,
        received: u32,
        jitter_ms: u32,
    },
}

// TODO:
pub const MEDIA_FRAME_HEADER_LEN: usize = 8;

pub fn encode_media_header(capture_ts_us: u64) -> [u8; MEDIA_FRAME_HEADER_LEN] {
    capture_ts_us.to_le_bytes()
}

pub fn decode_media_header(buf: &[u8]) -> Option<u64> {
    let bytes: [u8; MEDIA_FRAME_HEADER_LEN] = buf.get(..MEDIA_FRAME_HEADER_LEN)?.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

pub mod track_names {
    pub const MICROPHONE: &str = "microphone";
    pub const CAMERA: &str = "camera";
    pub const SCREEN: &str = "screen";
    pub const SCREEN_AUDIO: &str = "screen-audio";
    pub const CTL_C2S: &str = "c2s";
    pub const CTL_S2C: &str = "s2c";
}

pub fn track_name_for_hint(hint: &MediaHint) -> &'static str {
    match hint {
        MediaHint::Audio => track_names::MICROPHONE,
        MediaHint::Video => track_names::CAMERA,
        MediaHint::ScreenAudio => track_names::SCREEN_AUDIO,
        MediaHint::ScreenVideo => track_names::SCREEN,
    }
}

pub fn priority_for_hint(hint: &MediaHint) -> u8 {
    match hint {
        MediaHint::Audio => 3,
        MediaHint::ScreenAudio => 2,
        MediaHint::Video => 1,
        MediaHint::ScreenVideo => 0,
    }
}
