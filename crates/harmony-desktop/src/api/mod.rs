pub mod account;

pub use harmony_api::{ContactAction, UserManager};

use iced::Color;

use std::sync::Arc;

use harmony_api::{
    AddContactOutcome, ClientEvent, ClientOptions, EncryptedClient, Event, HarmonyClient,
    PublicUser, RelationshipState,
};
use reqwest::Client;
use rkyv::{Archive, Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::{
    MessageAuthor,
    errors::{RenderableError, RenderableResult},
};

#[derive(Archive, Serialize, Deserialize)]
pub struct GroupChannelMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}
#[derive(Debug, Clone)]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
    pub username: String,
    // FIXME: placeholder
    pub avatar_color_start: Color,
    pub avatar_color_end: Color,
}

impl From<PublicUser> for UserProfile {
    fn from(user: PublicUser) -> Self {
        use sha2::{Digest, Sha256};
        // FIXME: placeholder
        let hash = Sha256::digest(user.id.as_bytes());
        let r = hash[0] as f32 / 255.0;
        let g = hash[1] as f32 / 255.0;
        let b = hash[2] as f32 / 255.0;
        let r2 = hash[3] as f32 / 255.0;
        let g2 = hash[4] as f32 / 255.0;
        let b2 = hash[5] as f32 / 255.0;
        UserProfile {
            id: user.id,
            display_name: user.display_name,
            username: user.username,
            avatar_color_start: Color::from_rgb(r, g, b),
            avatar_color_end: Color::from_rgb(r2, g2, b2),
        }
    }
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
    PendingRemote,
    PendingLocal,
    None,
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
    pub session_id: String,
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

#[derive(Clone)]
pub struct ApiClient {
    crypto: Arc<EncryptedClient>,
    user_id: String,
    users: Arc<UserManager>,
}

impl ApiClient {
    pub async fn connect(
        as_url: &str,
        harmony_url: &str,
        token: &str,
        encrypted_key: &str,
        password: &str,
    ) -> Result<(Arc<ApiClient>, broadcast::Receiver<ClientEvent>), RenderableError> {
        let (client, recv) = HarmonyClient::new(ClientOptions::new(harmony_url, token)).await?;
        let crypto =
            EncryptedClient::connect(client, encrypted_key.to_string(), password.to_string())
                .await?;
        let user_id = crypto.user_id().to_string();
        let users = Arc::new(UserManager::new(Client::new(), as_url, token));
        let live = Self {
            crypto,
            user_id,
            users,
        };

        Ok((Arc::new(live), recv))
    }

    pub async fn call_identity(&self) -> pulse_api::MlsIdentity {
        let seed = self.crypto.identity_seed().await;
        let trusted = self.crypto.identity_key_snapshot().await;
        pulse_api::MlsIdentity {
            user_id: self.user_id.clone(),
            signing_seed: *seed,
            trusted_keys: Arc::new(move |user_id: &str| trusted.get(user_id).copied()),
        }
    }

    pub async fn handle_event(&self, event: &Event) -> RenderableResult<Option<Contact>> {
        let outcome = self.crypto.handle_event(event).await?;
        let contact = match outcome {
            Some(AddContactOutcome::Response(resp)) => {
                let profile = self
                    .users
                    .get_user(&resp.profile.id)
                    .await
                    .map(UserProfile::from)
                    .unwrap_or_else(|_| placeholder_profile(&resp.profile.id));
                Some(Contact {
                    profile,
                    status: map_relationship(&resp.state),
                })
            }
            Some(AddContactOutcome::Established { user_id }) => {
                let profile = self
                    .users
                    .get_user(&user_id)
                    .await
                    .map(UserProfile::from)
                    .unwrap_or_else(|_| placeholder_profile(&user_id));
                Some(Contact {
                    profile,
                    status: ContactStatus::Established,
                })
            }
            None => None,
        };
        Ok(contact)
    }

    pub async fn decrypt_content(&self, msg: &harmony_api::Message) -> RenderableResult<String> {
        let bytes = self.crypto.decrypt_content(msg).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub async fn map_message(&self, msg: &harmony_api::Message) -> RenderableResult<ApiMessage> {
        let text = self.decrypt_content(msg).await?;
        let profile = self
            .users
            .get_user(&msg.author_id)
            .await
            .map(UserProfile::from)
            .unwrap_or_else(|_| placeholder_profile(&msg.author_id));
        Ok(ApiMessage {
            id: msg.id.clone(),
            author: MessageAuthor::User {
                id: msg.author_id.clone(),
                name: profile.display_name,
                avatar_color_start: profile.avatar_color_start,
                avatar_color_end: profile.avatar_color_end,
            },
            content: ApiMessageContent::Text(text),
        })
    }

    pub async fn get_user_profile(&self, user_id: &str) -> RenderableResult<UserProfile> {
        self.users
            .get_user(user_id)
            .await
            .map(UserProfile::from)
            .map_err(|_| RenderableError::NetworkError)
    }

    pub async fn get_user_profile_by_username(
        &self,
        username: &str,
    ) -> RenderableResult<UserProfile> {
        self.users
            .get_user_by_username(username)
            .await
            .map(UserProfile::from)
            .map_err(|_| RenderableError::NetworkError)
    }

    pub async fn get_user_profiles(
        &self,
        user_ids: Vec<String>,
    ) -> RenderableResult<Vec<UserProfile>> {
        self.users
            .get_users(user_ids.clone())
            .await
            .map(|users| users.into_iter().map(UserProfile::from).collect())
            .map_err(|_| RenderableError::NetworkError)
    }

    pub async fn get_current_user(&self) -> RenderableResult<CurrentUser> {
        let resp = self.crypto.client().get_current_user().await?;
        let profile = self
            .users
            .get_user(&resp.id)
            .await
            .map(UserProfile::from)
            .unwrap_or_else(|_| placeholder_profile(&resp.id));
        let status = match resp.presence.status {
            harmony_api::Status::Online => UserStatus::Online,
            harmony_api::Status::Idle => UserStatus::Away,
            harmony_api::Status::Busy | harmony_api::Status::BusyNotify => UserStatus::DoNotDisturb,
            harmony_api::Status::Offline => UserStatus::Offline,
        };
        Ok(CurrentUser {
            profile,
            status,
            email: String::new(),
        })
    }

    pub async fn get_conversations(&self) -> RenderableResult<Vec<Channel>> {
        let channels = self.crypto.client().get_channels().await?;
        let mut result = Vec::with_capacity(channels.len());

        for ch in &channels {
            match ch {
                harmony_api::Channel::PrivateChannel {
                    id,
                    initiator_id,
                    target_id,
                    ..
                } => {
                    let other_id = if *initiator_id == self.user_id {
                        target_id
                    } else {
                        initiator_id
                    };
                    let profile = self
                        .users
                        .get_user(other_id)
                        .await
                        .map(UserProfile::from)
                        .map_err(|_| RenderableError::NetworkError)?;
                    result.push(Channel::Private {
                        id: id.clone(),
                        other: profile,
                    });
                }
                harmony_api::Channel::GroupChannel { id, members, .. } => {
                    let member_ids: Vec<String> = members.iter().map(|m| m.id.clone()).collect();
                    let profiles: Vec<UserProfile> = self
                        .users
                        .get_users(member_ids.clone())
                        .await
                        .map_err(|_| RenderableError::NetworkError)?
                        .into_iter()
                        .map(UserProfile::from)
                        .collect();
                    result.push(Channel::Group {
                        id: id.clone(),
                        name: None,
                        participants: profiles,
                    });
                }
            }
        }

        Ok(result)
    }

    pub async fn get_messages(&self, channel_id: &str) -> RenderableResult<Vec<ApiMessage>> {
        let messages = self
            .crypto
            .client()
            .get_messages(channel_id, Some(50), None, None, None)
            .await?;

        let mut result = Vec::with_capacity(messages.len());
        for msg in &messages {
            result.push(self.map_message(msg).await?);
        }
        Ok(result)
    }

    pub async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> RenderableResult<ApiMessage> {
        let encrypted = self
            .crypto
            .encrypt_content(channel_id, content.as_bytes())
            .await?;
        let msg = self
            .crypto
            .client()
            .send_message(channel_id, encrypted)
            .await?;
        let profile = self
            .users
            .get_user(&self.user_id)
            .await
            .map(UserProfile::from)
            .unwrap_or_else(|_| placeholder_profile(&self.user_id));
        Ok(ApiMessage {
            id: msg.id,
            author: MessageAuthor::User {
                id: self.user_id.clone(),
                name: profile.display_name,
                avatar_color_start: profile.avatar_color_start,
                avatar_color_end: profile.avatar_color_end,
            },
            content: ApiMessageContent::Text(content.to_string()),
        })
    }

    pub async fn edit_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> RenderableResult<ApiMessage> {
        let encrypted = self
            .crypto
            .encrypt_content(channel_id, content.as_bytes())
            .await?;
        let msg = self
            .crypto
            .client()
            .edit_message(message_id, encrypted)
            .await?;
        let profile = self
            .users
            .get_user(&msg.author_id)
            .await
            .map(UserProfile::from)
            .map_err(|_| RenderableError::NetworkError)?;
        Ok(ApiMessage {
            id: msg.id,
            author: MessageAuthor::User {
                id: msg.author_id.clone(),
                name: profile.display_name,
                avatar_color_start: profile.avatar_color_start,
                avatar_color_end: profile.avatar_color_end,
            },
            content: ApiMessageContent::Text(content.to_string()),
        })
    }

    pub async fn delete_message(&self, message_id: &str) -> RenderableResult<()> {
        self.crypto.client().delete_message(message_id).await?;
        Ok(())
    }

    pub async fn get_call(&self, channel_id: &str) -> RenderableResult<Option<CallState>> {
        let members = self.crypto.client().get_call_members(channel_id).await?;
        let member_ids: Vec<String> = members.iter().map(|m| m.user_id.clone()).collect();
        let profiles: Vec<UserProfile> = self
            .users
            .get_users(member_ids.clone())
            .await
            .map_err(|_| RenderableError::NetworkError)?
            .into_iter()
            .map(UserProfile::from)
            .collect();
        let participants = members
            .iter()
            .zip(profiles.into_iter())
            .map(|(m, profile)| CallParticipant {
                profile,
                session_id: m.session_id.clone(),
                tracks: CallTrackState {
                    audio: !m.muted,
                    video: false,
                    screen: false,
                },
            })
            .collect();
        Ok(Some(CallState { participants }))
    }

    pub async fn start_call(&self, channel_id: &str) -> RenderableResult<()> {
        self.crypto.client().start_call(channel_id, None).await?;
        Ok(())
    }

    pub async fn create_call_token(&self, channel_id: &str) -> RenderableResult<CallTokenInfo> {
        let resp = self
            .crypto
            .client()
            .create_call_token(channel_id, true, false)
            .await?;
        Ok(CallTokenInfo {
            session_id: resp.id,
            token: resp.token,
            server_address: resp.server_address,
            call_id: resp.call_id,
        })
    }

    pub async fn update_voice_state(
        &self,
        channel_id: &str,
        muted: Option<bool>,
        deafened: Option<bool>,
    ) -> RenderableResult<()> {
        self.crypto
            .client()
            .update_voice_state(channel_id, muted, deafened)
            .await?;
        Ok(())
    }

    pub async fn get_contacts(&self) -> RenderableResult<Vec<Contact>> {
        let contacts = self.crypto.client().get_contacts().await?;
        let mut result = Vec::with_capacity(contacts.len());
        for c in contacts {
            let profile = self
                .users
                .get_user(&c.id)
                .await
                .map(UserProfile::from)
                .map_err(|_| RenderableError::NetworkError)?;
            let status = map_relationship(&c.state);
            result.push(Contact { profile, status });
        }
        Ok(result)
    }

    pub async fn add_contact(&self, action: ContactAction) -> RenderableResult<Contact> {
        match self.crypto.add_contact(action).await? {
            AddContactOutcome::Response(resp) => {
                let profile = self
                    .users
                    .get_user(&resp.profile.id)
                    .await
                    .map(UserProfile::from)
                    .map_err(|_| {
                        RenderableError::UnknownError("Contact not found after operation".into())
                    })?;
                Ok(Contact {
                    profile,
                    status: map_relationship(&resp.state),
                })
            }
            AddContactOutcome::Established { user_id } => {
                let profile = self
                    .users
                    .get_user(&user_id)
                    .await
                    .map(UserProfile::from)
                    .unwrap_or_else(|_| placeholder_profile(&user_id));
                Ok(Contact {
                    profile,
                    status: ContactStatus::Established,
                })
            }
        }
    }

    pub async fn remove_contact(&self, user_id: &str) -> RenderableResult<()> {
        self.crypto.client().remove_contact(user_id).await?;
        Ok(())
    }

    pub async fn block_contact(&self, user_id: &str) -> RenderableResult<()> {
        self.crypto.client().block_contact(user_id).await?;
        Ok(())
    }

    pub async fn unblock_contact(&self, user_id: &str) -> RenderableResult<Contact> {
        let c = self.crypto.client().unblock_contact(user_id).await?;
        let profile = self
            .users
            .get_user(&c.id)
            .await
            .map(UserProfile::from)
            .map_err(|_| RenderableError::NetworkError)?;
        Ok(Contact {
            profile,
            status: map_relationship(&c.state),
        })
    }

    pub async fn create_private_channel(&self, user_id: &str) -> RenderableResult<Channel> {
        let api_channel = self.crypto.client().create_private_channel(user_id).await?;
        let profile = self
            .users
            .get_user(user_id)
            .await
            .map(UserProfile::from)
            .map_err(|_| RenderableError::NetworkError)?;
        Ok(Channel::Private {
            id: api_channel.id().to_string(),
            other: profile,
        })
    }

    async fn create_group_channel(
        &self,
        name: Option<&str>,
        description: Option<&str>,
    ) -> RenderableResult<Channel> {
        let metadata = GroupChannelMetadata {
            name: name.map(|s| s.to_string()),
            description: description.map(|s| s.to_string()),
        };
        let metadata_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&metadata)
            .expect("serialization should not fail")
            .into_vec();
        let api_channel = self.crypto.create_group_channel(&metadata_bytes).await?;
        let channel_id = api_channel.id().to_string();
        Ok(Channel::Group {
            id: channel_id,
            name: metadata.name,
            participants: vec![
                self.users
                    .get_user(&self.user_id)
                    .await
                    .map(UserProfile::from)
                    .unwrap_or_else(|_| placeholder_profile(&self.user_id)),
            ],
        })
    }

    async fn get_group_key(&self, channel_id: &str) -> RenderableResult<Option<Vec<u8>>> {
        Ok(self.crypto.get_group_key(channel_id).await)
    }

    async fn create_group_invite(&self, channel_id: &str) -> RenderableResult<String> {
        Ok(self.crypto.create_group_invite(channel_id).await?)
    }

    async fn join_group(&self, invite_code: &str, group_key: &[u8]) -> RenderableResult<()> {
        self.crypto.join_group(invite_code, group_key).await?;
        Ok(())
    }
}

pub fn map_relationship(r: &harmony_api::RelationshipState) -> ContactStatus {
    match r {
        RelationshipState::Established { .. } => ContactStatus::Established,
        RelationshipState::Blocked => ContactStatus::Blocked,
        RelationshipState::Requested { public_key: None } => ContactStatus::PendingRemote,
        RelationshipState::Requested { .. } => ContactStatus::PendingLocal,
        RelationshipState::PendingKeyExchange { .. } => ContactStatus::PendingRemote,
        RelationshipState::None => ContactStatus::None,
    }
}
