pub mod account;
pub mod crypto;
pub mod live;
pub mod mock;
pub mod user_manager;

pub use user_manager::UserManager;

use async_trait::async_trait;
use iced::Color;

use crate::MessageAuthor;
use crate::errors::RenderableResult;

#[derive(Debug, Clone)]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
    pub username: String,
    // FIXME: placeholder
    pub avatar_color_start: Color,
    pub avatar_color_end: Color,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserStatus {
    Online,
    Away,
    DoNotDisturb,
    Offline,
}

#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub profile: UserProfile,
    pub status: UserStatus,
    pub email: String,
}

#[derive(Debug, Clone)]
pub enum ApiMessageContent {
    Text(String),
    CallCard { channel: String, duration: String },
}

#[derive(Debug, Clone)]
pub struct ApiMessage {
    pub id: String,
    pub author: MessageAuthor,
    pub content: ApiMessageContent,
}

#[derive(Debug, Clone)]
pub enum Channel {
    Private {
        id: String,
        other: UserProfile,
    },
    Group {
        id: String,
        name: Option<String>,
        participants: Vec<UserProfile>,
    },
}

impl Channel {
    pub fn id(&self) -> String {
        match self {
            Channel::Private { id, .. } => id.clone(),
            Channel::Group { id, .. } => id.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactStatus {
    Established,
    Pending,
    Requested,
    Blocked,
}

#[derive(Debug, Clone)]
pub struct Contact {
    pub profile: UserProfile,
    pub status: ContactStatus,
}

#[derive(Debug, Clone)]
pub struct CallTrackState {
    pub audio: bool,
    pub video: bool,
    pub screen: bool,
}

#[derive(Debug, Clone)]
pub struct CallParticipant {
    pub profile: UserProfile,
    pub tracks: CallTrackState,
}

#[derive(Debug, Clone)]
pub struct CallState {
    pub participants: Vec<CallParticipant>,
}

#[derive(Debug, Clone)]
pub struct CallTokenInfo {
    pub session_id: String,
    pub token: String,
    pub server_address: String,
    pub call_id: String,
}

#[async_trait]
pub trait ApiClient: Send + Sync {
    async fn get_current_user(&self) -> RenderableResult<CurrentUser>;
    async fn get_conversations(&self) -> RenderableResult<Vec<Channel>>;
    async fn get_messages(&self, conversation_id: String) -> RenderableResult<Vec<ApiMessage>>;
    async fn send_message(
        &self,
        channel_id: String,
        content: String,
    ) -> RenderableResult<ApiMessage>;
    async fn edit_message(
        &self,
        message_id: String,
        channel_id: String,
        content: String,
    ) -> RenderableResult<ApiMessage>;
    async fn delete_message(&self, message_id: String) -> RenderableResult<()>;
    async fn get_call(&self, channel_id: String) -> RenderableResult<Option<CallState>>;
    async fn start_call(&self, channel_id: String) -> RenderableResult<()>;
    async fn create_call_token(&self, channel_id: String) -> RenderableResult<CallTokenInfo>;
    async fn update_voice_state(
        &self,
        channel_id: String,
        muted: Option<bool>,
        deafened: Option<bool>,
    ) -> RenderableResult<()>;
    async fn get_contacts(&self) -> RenderableResult<Vec<Contact>>;
    async fn add_contact(&self, username: String) -> RenderableResult<Contact>;
    async fn remove_contact(&self, user_id: String) -> RenderableResult<()>;
    async fn accept_contact(&self, user_id: String) -> RenderableResult<Contact>;
    async fn block_contact(&self, user_id: String) -> RenderableResult<()>;
    async fn unblock_contact(&self, user_id: String) -> RenderableResult<Contact>;
    async fn get_user_profile(&self, user_id: String) -> RenderableResult<UserProfile> {
        Ok(placeholder_profile(&user_id))
    }
    async fn get_user_profiles(&self, user_ids: Vec<String>) -> RenderableResult<Vec<UserProfile>> {
        let mut profiles = Vec::with_capacity(user_ids.len());
        for id in &user_ids {
            profiles.push(self.get_user_profile(id.clone()).await?);
        }
        Ok(profiles)
    }
}

pub fn placeholder_profile(user_id: &str) -> UserProfile {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(user_id.as_bytes());
    let r = hash[0] as f32 / 255.0;
    let g = hash[1] as f32 / 255.0;
    let b = hash[2] as f32 / 255.0;
    let r2 = hash[3] as f32 / 255.0;
    let g2 = hash[4] as f32 / 255.0;
    let b2 = hash[5] as f32 / 255.0;
    UserProfile {
        id: user_id.to_string(),
        display_name: "Unknown user".to_string(),
        username: "?".to_string(),
        avatar_color_start: Color::from_rgb(r, g, b),
        avatar_color_end: Color::from_rgb(r2, g2, b2),
    }
}

pub use mock::MockApiClient;
