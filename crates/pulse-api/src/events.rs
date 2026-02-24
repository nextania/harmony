use pulse_types::AvailableTrack;

/// Events emitted by `PulseClient` for the consumer to handle.
///
/// MLS coordination (`MlsProposals`, `MlsCommit`, `InitializeGroup`) and heartbeats
/// are handled internally by the client and are not surfaced here.
#[derive(Clone, Debug)]
pub enum PulseEvent {
    /// Successfully connected to the call. Contains the assigned session ID
    /// and a list of tracks already being produced by other participants.
    Connected {
        id: String,
        available_tracks: Vec<AvailableTrack>,
    },

    /// Disconnected from the server. If `reconnect` is `Some`, the client
    /// should reconnect to the provided `(server_address, token)`.
    Disconnected { reconnect: Option<(String, String)> },

    /// A new track from another participant has become available for consumption.
    TrackAvailable(AvailableTrack),

    /// A previously available track is no longer available.
    TrackUnavailable(String),

    /// A new MLS epoch has been reached by all members.
    EpochReady(u64),
}
