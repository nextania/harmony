pub mod account;

use iced::Color;

use std::{collections::HashMap, sync::Arc};

use harmony_api::{
    AddContactOutcome, ClientEvent, ClientOptions, ContactAction, EncryptedClient, Event,
    HarmonyClient, PublicUser, UserManager,
};
use reqwest::Client;
use rkyv::{Archive, Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::errors::{RenderableError, RenderableResult};

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
        let (start, end) = avatar_colors(&user.id);
        UserProfile {
            id: user.id,
            display_name: user.display_name,
            username: user.username,
            avatar_color_start: start,
            avatar_color_end: end,
        }
    }
}

// FIXME: placeholder
fn avatar_colors(user_id: &str) -> (Color, Color) {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(user_id.as_bytes());
    let c = |i: usize| {
        Color::from_rgb(
            hash[i] as f32 / 255.0,
            hash[i + 1] as f32 / 255.0,
            hash[i + 2] as f32 / 255.0,
        )
    };
    (c(0), c(3))
}

pub fn placeholder_profile(user_id: &str) -> UserProfile {
    let (start, end) = avatar_colors(user_id);
    UserProfile {
        id: user_id.to_string(),
        display_name: "Unknown user".to_string(),
        username: "?".to_string(),
        avatar_color_start: start,
        avatar_color_end: end,
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

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn client(&self) -> &HarmonyClient {
        self.crypto.client()
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

    pub async fn handle_event(&self, event: &Event) -> RenderableResult<Option<AddContactOutcome>> {
        Ok(self.crypto.handle_event(event).await?)
    }

    pub async fn add_contact(&self, action: ContactAction) -> RenderableResult<AddContactOutcome> {
        Ok(self.crypto.add_contact(action).await?)
    }

    pub async fn decrypt_message(&self, msg: &harmony_api::Message) -> RenderableResult<String> {
        let bytes = self.crypto.decrypt_content(msg).await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    pub async fn get_profile(&self, user_id: &str) -> RenderableResult<UserProfile> {
        Ok(self.users.get_user(user_id).await.map(UserProfile::from)?)
    }

    pub async fn get_profile_by_username(&self, username: &str) -> RenderableResult<UserProfile> {
        Ok(self
            .users
            .get_user_by_username(username)
            .await
            .map(UserProfile::from)?)
    }

    pub async fn get_profiles(&self, user_ids: Vec<String>) -> RenderableResult<Vec<UserProfile>> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self
            .users
            .get_users(user_ids)
            .await?
            .into_iter()
            .map(UserProfile::from)
            .collect())
    }

    pub async fn get_messages(
        &self,
        channel_id: &str,
    ) -> RenderableResult<Vec<(harmony_api::Message, String)>> {
        let messages = self
            .crypto
            .client()
            .get_messages(channel_id, Some(50), None, None, None)
            .await?;

        let mut result = Vec::with_capacity(messages.len());
        for msg in messages {
            let text = self.decrypt_message(&msg).await?;
            result.push((msg, text));
        }
        Ok(result)
    }

    pub async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> RenderableResult<harmony_api::Message> {
        let encrypted = self
            .crypto
            .encrypt_content(channel_id, content.as_bytes())
            .await?;
        Ok(self
            .crypto
            .client()
            .send_message(channel_id, encrypted)
            .await?)
    }

    pub async fn edit_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> RenderableResult<harmony_api::Message> {
        let encrypted = self
            .crypto
            .encrypt_content(channel_id, content.as_bytes())
            .await?;
        Ok(self
            .crypto
            .client()
            .edit_message(message_id, encrypted)
            .await?)
    }

    async fn create_group_channel(
        &self,
        name: Option<&str>,
        description: Option<&str>,
    ) -> RenderableResult<harmony_api::Channel> {
        let metadata = GroupChannelMetadata {
            name: name.map(|s| s.to_string()),
            description: description.map(|s| s.to_string()),
        };
        let metadata_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&metadata)
            .expect("serialization should not fail")
            .into_vec();
        Ok(self.crypto.create_group_channel(&metadata_bytes).await?)
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

pub async fn load_session(
    client: &ApiClient,
) -> RenderableResult<(
    HashMap<String, harmony_api::Channel>,
    HashMap<String, UserProfile>,
)> {
    let channels = client.client().get_channels().await?;

    let mut user_ids = vec![client.user_id().to_string()];
    for ch in &channels {
        match ch {
            harmony_api::Channel::PrivateChannel {
                initiator_id,
                target_id,
                ..
            } => {
                user_ids.push(initiator_id.clone());
                user_ids.push(target_id.clone());
            }
            harmony_api::Channel::GroupChannel { members, .. } => {
                user_ids.extend(members.iter().map(|m| m.id.clone()));
            }
        }
    }
    user_ids.sort();
    user_ids.dedup();

    let profiles = client
        .get_profiles(user_ids)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    let conversations = channels
        .into_iter()
        .map(|c| (c.id().to_string(), c))
        .collect();
    Ok((conversations, profiles))
}
