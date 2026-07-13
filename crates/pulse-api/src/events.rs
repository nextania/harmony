use pulse_types::{AvailableTrack, MediaHint};

use crate::error::PulseError;

/// One authenticated member of the call's MLS group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallMember {
    pub session_id: String,
    pub user_id: String,
    pub state: CallMemberState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallMemberState {
    Verified,
    Unverified,
    Warning,
}

/// Events emitted by `PulseClient` for the consumer to handle.
///
/// MLS coordination (`MlsProposals`, `MlsCommit`, `InitializeGroup`) and heartbeats
/// are handled internally by the client and are not surfaced here.
#[derive(Clone, Debug)]
pub enum PulseEvent {
    Connected {
        id: String,
        available_tracks: Vec<AvailableTrack>,
    },
    Reconnecting {
        attempt: u32,
    },
    Disconnected {
        reason: String,
    },

    TrackAvailable(AvailableTrack),
    TrackUnavailable(String),
    EpochReady(u64),
    // TODO:
    MembershipChanged {
        epoch: u64,
        members: Vec<CallMember>,
    },

    KeyFrameRequested(MediaHint),
    ReceiverReport {
        media_hint: MediaHint,
        lost: u32,
        received: u32,
        jitter_ms: u32,
    },

    Error(PulseError),
}
