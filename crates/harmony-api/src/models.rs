use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Status {
    Online = 0,
    Idle = 1,
    Busy = 2,
    BusyNotify = 3,
    Invisible = 4,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Presence {
    pub status: Status,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Relationship {
    Established = 0,
    Blocked = 1,
    Requested = 2,
    Pending = 3,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Contact {
    pub id: String,
    pub relationship: Relationship,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContactExtended {
    pub id: String,
    pub relationship: Relationship,
    pub user: User,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMember {
    pub id: String,
    pub role: ChannelMemberRole,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelMemberRole {
    Member,
    Manager,
}

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
    pub fn id(&self) -> &str {
        match self {
            Channel::PrivateChannel { id, .. } => id,
            Channel::GroupChannel { id, .. } => id,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Channel::PrivateChannel { target_id, .. } => {
                format!("Private chat with {}", target_id)
            }
            Channel::GroupChannel { name, .. } => name.clone(),
        }
    }
}

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
    pub fn created_at_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.created_at).unwrap_or_default()
    }

    pub fn edited_at_datetime(&self) -> Option<DateTime<Utc>> {
        self.edited_at.and_then(DateTime::from_timestamp_millis)
    }
}

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
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = chrono::Utc::now().timestamp_millis() as u64;
            now > expires_at
        } else {
            false
        }
    }

    pub fn is_max_uses_reached(&self) -> bool {
        if let Some(max_uses) = self.max_uses {
            self.uses.len() >= max_uses as usize
        } else {
            false
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.is_expired() && !self.is_max_uses_reached()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StartCallResponse {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCallTokenResponse {
    pub id: String,
    pub token: String,
    pub server_address: String,
    pub call_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdateVoiceStateResponse {
    pub muted: bool,
    pub deafened: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallMember {
    pub user_id: String,
    pub session_id: String,
    pub muted: bool,
    pub deafened: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GetCallMembersResponse {
    pub members: Vec<CallMember>,
}
