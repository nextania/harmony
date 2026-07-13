//! # Pulse API Client Library
//!
//! A Rust client library for interacting with the Harmony's voice server, Pulse.

mod client;
mod error;
mod events;
mod mls;

pub use client::{MediaFrame, PulseClient, PulseClientOptions, TrackHandle};
pub use error::PulseError;
pub use events::{CallMember, PulseEvent};
pub use mls::{IdentityKeyResolver, MlsIdentity};

pub use pulse_types::{AvailableTrack, MediaHint};
