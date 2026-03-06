use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EncryptionHint {
    /// Strongest: forward secrecy via MLS, messages are not persistent.
    Mls,
    /// Less secure (no forward secrecy) but messages can be stored server-side.
    Persistent,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelMemberRole {
    Member,
    Manager,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMember {
    pub id: String,
    pub role: ChannelMemberRole,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Channel {
    PrivateChannel {
        id: String,
        initiator_id: String,
        target_id: String,
        last_key_id: String,
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
pub struct GetChannelMethod {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelResponse {
    pub channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelsMethod {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelsResponse {
    pub channels: Vec<Channel>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelInformation {
    #[serde(rename_all = "camelCase")]
    PrivateChannel { target_id: String },
    #[serde(rename_all = "camelCase")]
    GroupChannel {
        metadata: Vec<u8>,
        encryption_hint: EncryptionHint,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateChannelMethod {
    pub channel: ChannelInformation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateChannelResponse {
    pub channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditChannelMethod {
    pub channel_id: String,
    pub metadata: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditChannelResponse {
    pub channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteChannelMethod {
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteChannelResponse {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveChannelMethod {
    pub channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveChannelResponse {}
