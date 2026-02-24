pub mod fragment;

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
pub struct WtFragmentedTrackData {
    pub id: String,
    pub sequence_id: u32,
    pub fragment_index: u16,
    pub fragment_count: u16,
    pub data: Vec<u8>,
}
