//! Data models for the Harmony API

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// User information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct User {
    pub id: String,
    pub profile_banner: Option<String>,
    pub profile_description: String,
    pub username: String,
    pub discriminator: String,
    pub profile_picture: Option<String>,
    pub presence: Option<Presence>,
    pub contacts: Vec<Contact>,
}

/// User online status
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Status {
    Online = 0,
    Idle = 1,
    Busy = 2,
    BusyNotify = 3,
    Invisible = 4,
}

/// User presence information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Presence {
    pub status: Status,
    pub message: String,
}

/// Relationship types between users
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Relationship {
    Established = 0,
    Blocked = 1,
    Requested = 2,
    Pending = 3,
}

/// Contact information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Contact {
    pub id: String,
    pub relationship: Relationship,
}

/// Extended contact information with user details
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContactExtended {
    pub id: String,
    pub relationship: Relationship,
    pub user: User,
}

/// Channel member information
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMember {
    pub id: String,
    pub role: ChannelMemberRole,
}

/// Channel member roles
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelMemberRole {
    Member,
    Manager,
}

/// Channel types
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Channel {
    PrivateChannel {
        id: String,
        initiator_id: String,
        target_id: String,
    },
    GroupChannel {
        id: String,
        name: String,
        description: String,
        members: Vec<ChannelMember>,
        blacklist: Vec<String>,
    },
}

impl Channel {
    /// Get the channel ID
    pub fn id(&self) -> &str {
        match self {
            Channel::PrivateChannel { id, .. } => id,
            Channel::GroupChannel { id, .. } => id,
        }
    }

    /// Get the channel name (for display purposes)
    pub fn name(&self) -> String {
        match self {
            Channel::PrivateChannel { target_id, .. } => {
                format!("Private chat with {}", target_id)
            }
            Channel::GroupChannel { name, .. } => name.clone(),
        }
    }
}

/// Message in a channel
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub content: String,
    pub author_id: String,
    pub created_at: i64,
    pub edited: bool,
    pub edited_at: Option<i64>,
    pub channel_id: String,
}

impl Message {
    /// Get the creation time as a DateTime
    pub fn created_at_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.created_at).unwrap_or_default()
    }

    /// Get the edit time as a DateTime (if edited)
    pub fn edited_at_datetime(&self) -> Option<DateTime<Utc>> {
        self.edited_at
            .and_then(|ts| DateTime::from_timestamp_millis(ts))
    }
}

/// Invite to a channel
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub code: String,
    pub channel_id: String,
    pub creator_id: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub max_uses: Option<i32>,
    pub uses: Vec<String>,
    pub authorized_users: Option<Vec<String>>,
}

impl Invite {
    /// Check if the invite has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = chrono::Utc::now().timestamp_millis() as u64;
            now > expires_at
        } else {
            false
        }
    }

    /// Check if the invite has reached max uses
    pub fn is_max_uses_reached(&self) -> bool {
        if let Some(max_uses) = self.max_uses {
            self.uses.len() >= max_uses as usize
        } else {
            false
        }
    }

    /// Check if the invite is still valid
    pub fn is_valid(&self) -> bool {
        !self.is_expired() && !self.is_max_uses_reached()
    }
}

/// Active call information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ActiveCall {
    pub id: String,
    pub channel_id: String,
    pub participants: Vec<String>,
    pub started_at: i64,
}

/// WebRTC authorization token
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RtcAuthorization {
    pub channel_id: String,
    pub user_id: String,
}
