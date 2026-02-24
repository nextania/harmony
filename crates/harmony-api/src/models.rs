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
pub struct UserProfile {
    pub id: String,
    pub public_key: Option<Vec<u8>>,
}

/// Current user profile returned by GET_CURRENT_USER.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentUserResponse {
    pub id: String,
    pub public_key: Option<Vec<u8>>,
    pub encrypted_keys: Option<Vec<u8>>,
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
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EncryptionHint {
    Mls,
    Persistent,
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
        metadata: Vec<u8>,
        members: Vec<ChannelMember>,
        pending_members: Vec<String>,
        blacklist: Vec<String>,
        encryption_hint: EncryptionHint,
    },
}

impl Channel {
    pub fn id(&self) -> &str {
        match self {
            Channel::PrivateChannel { id, .. } => id,
            Channel::GroupChannel { id, .. } => id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub content: Vec<u8>,
    pub author_id: String,
    pub edited_at: Option<i64>,
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub code: String,
    pub channel_id: String,
    pub creator: String,
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
