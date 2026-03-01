use std::sync::Arc;

use async_trait::async_trait;
use harmony_api::{ClientOptions, Event, HarmonyClient};
use reqwest::Client;
use tokio::sync::{Mutex, mpsc};

use crate::{
    MessageAuthor,
    api::{
        ApiClient, ApiMessage, ApiMessageContent, CallParticipant, CallState, CallTokenInfo,
        CallTrackState, Channel, Contact, ContactStatus, CurrentUser, UserManager, UserProfile,
        UserStatus, crypto::PersistentEncryption, placeholder_profile,
    },
    errors::{RenderableError, RenderableResult},
};

#[derive(Clone)]
pub struct LiveApiClient {
    client: HarmonyClient,
    crypto: Arc<Mutex<PersistentEncryption>>,
    user_id: String,
    users: Arc<UserManager>,
}

impl LiveApiClient {
    pub async fn connect(
        url: &str,
        token: &str,
    ) -> Result<(Arc<dyn ApiClient>, mpsc::UnboundedReceiver<Event>), RenderableError> {
        let (client, recv) = HarmonyClient::new(ClientOptions::new(url, token)).await?;

        let current = client.get_current_user().await?;

        let crypto = if let Some(ref encrypted_keys) = current.encrypted_keys {
            // FIXME: decrypt with password-derived key, then deserialize
            if encrypted_keys.len() >= 32 {
                let mut secret = [0u8; 32];
                secret.copy_from_slice(&encrypted_keys[..32]);
                PersistentEncryption::from_secret_bytes(secret)
            } else {
                PersistentEncryption::generate()
            }
        } else {
            let enc = PersistentEncryption::generate();
            let _ = client
                .set_key_package(
                    enc.public_key_bytes().to_vec(),
                    enc.secret_key_bytes().to_vec(), // TODO: encrypt with password-derived key
                )
                .await;
            enc
        };

        let users = UserManager::new(Client::new(), url, token);

        let live = Self {
            client,
            crypto: Arc::new(Mutex::new(crypto)),
            user_id: current.id.clone(),
            users,
        };

        Ok((Arc::new(live), recv))
    }

    async fn channel_peer_key(&self, channel: &harmony_api::Channel) -> Option<[u8; 32]> {
        match channel {
            harmony_api::Channel::PrivateChannel {
                initiator_id,
                target_id,
                ..
            } => {
                let other_id = if *initiator_id == self.user_id {
                    target_id
                } else {
                    initiator_id
                };
                if let Ok(profile) = self.client.get_user(other_id).await {
                    profile
                        .public_key
                        .as_ref()
                        .and_then(|k| <[u8; 32]>::try_from(k.as_slice()).ok())
                } else {
                    None
                }
            }
            harmony_api::Channel::GroupChannel { .. } => {
                // TODO: persistent group encryption key derivation
                None
            }
        }
    }

    async fn peer_key_for_channel_id(&self, channel_id: &str) -> Option<[u8; 32]> {
        if let Ok(channel) = self.client.get_channel(channel_id).await {
            self.channel_peer_key(&channel).await
        } else {
            None
        }
    }

    pub async fn decrypt_content(&self, content: &[u8], channel_id: &str) -> String {
        if content.is_empty() {
            return String::new();
        }
        if let Some(peer_key) = self.peer_key_for_channel_id(channel_id).await {
            let mut crypto = self.crypto.lock().await;
            match crypto.decrypt(content, &peer_key) {
                Ok(plaintext) => String::from_utf8_lossy(&plaintext).into_owned(),
                Err(_) => String::from_utf8_lossy(content).into_owned(),
            }
        } else {
            String::from_utf8_lossy(content).into_owned()
        }
    }

    async fn encrypt_content(&self, plaintext: &str, channel_id: &str) -> Vec<u8> {
        if let Some(peer_key) = self.peer_key_for_channel_id(channel_id).await {
            let mut crypto = self.crypto.lock().await;
            crypto.encrypt(plaintext.as_bytes(), &peer_key)
        } else {
            plaintext.as_bytes().to_vec()
        }
    }

    async fn map_message(&self, msg: &harmony_api::Message) -> ApiMessage {
        let text = self.decrypt_content(&msg.content, &msg.channel_id).await;
        let profile = self
            .users
            .get_user(&msg.author_id)
            .await
            .unwrap_or_else(|_| placeholder_profile(&msg.author_id));
        ApiMessage {
            id: msg.id.clone(),
            author: MessageAuthor::User {
                id: msg.author_id.clone(),
                name: profile.display_name,
                avatar_color_start: profile.avatar_color_start,
                avatar_color_end: profile.avatar_color_end,
            },
            content: ApiMessageContent::Text(text),
        }
    }
}

fn map_relationship(r: harmony_api::Relationship) -> ContactStatus {
    match r {
        harmony_api::Relationship::Established => ContactStatus::Established,
        harmony_api::Relationship::Blocked => ContactStatus::Blocked,
        harmony_api::Relationship::Requested => ContactStatus::Requested,
        harmony_api::Relationship::Pending => ContactStatus::Pending,
    }
}

#[async_trait]
impl ApiClient for LiveApiClient {
    async fn get_user_profile(&self, user_id: String) -> RenderableResult<UserProfile> {
        self.users
            .get_user(&user_id)
            .await
            .map_err(|_| RenderableError::NetworkError)
    }

    async fn get_user_profiles(&self, user_ids: Vec<String>) -> RenderableResult<Vec<UserProfile>> {
        self.users
            .get_users(user_ids.clone())
            .await
            .map_err(|_| RenderableError::NetworkError)
    }

    async fn get_current_user(&self) -> RenderableResult<CurrentUser> {
        let resp = self.client.get_current_user().await?;
        let profile = self
            .users
            .get_user(&resp.id)
            .await
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

    async fn get_conversations(&self) -> RenderableResult<Vec<Channel>> {
        let channels = self.client.get_channels().await?;
        let mut result = Vec::with_capacity(channels.len());

        for ch in &channels {
            match ch {
                harmony_api::Channel::PrivateChannel {
                    id,
                    initiator_id,
                    target_id,
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
                        .map_err(|_| RenderableError::NetworkError)?;
                    result.push(Channel::Private {
                        id: id.clone(),
                        other: profile,
                    });
                }
                harmony_api::Channel::GroupChannel { id, members, .. } => {
                    let member_ids: Vec<String> = members.iter().map(|m| m.id.clone()).collect();
                    let profiles = self
                        .users
                        .get_users(member_ids.clone())
                        .await
                        .map_err(|_| RenderableError::NetworkError)?;
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

    async fn get_messages(&self, channel_id: String) -> RenderableResult<Vec<ApiMessage>> {
        let messages = self
            .client
            .get_messages(&channel_id, Some(50), Some(true), None, None)
            .await?;

        let mut result = Vec::with_capacity(messages.len());
        for msg in &messages {
            result.push(self.map_message(msg).await);
        }
        Ok(result)
    }

    async fn send_message(
        &self,
        channel_id: String,
        content: String,
    ) -> RenderableResult<ApiMessage> {
        let encrypted = self.encrypt_content(&content, &channel_id).await;
        let msg = self.client.send_message(&channel_id, encrypted).await?;
        let profile = self
            .users
            .get_user(&self.user_id)
            .await
            .unwrap_or_else(|_| placeholder_profile(&self.user_id));
        Ok(ApiMessage {
            id: msg.id,
            author: MessageAuthor::User {
                id: self.user_id.clone(),
                name: profile.display_name,
                avatar_color_start: profile.avatar_color_start,
                avatar_color_end: profile.avatar_color_end,
            },
            content: ApiMessageContent::Text(content),
        })
    }

    async fn edit_message(
        &self,
        message_id: String,
        channel_id: String,
        content: String,
    ) -> RenderableResult<ApiMessage> {
        let encrypted = self.encrypt_content(&content, &channel_id).await;
        let msg = self.client.edit_message(&message_id, encrypted).await?;
        let profile = self
            .users
            .get_user(&msg.author_id)
            .await
            .map_err(|_| RenderableError::NetworkError)?;
        Ok(ApiMessage {
            id: msg.id,
            author: MessageAuthor::User {
                id: msg.author_id.clone(),
                name: profile.display_name,
                avatar_color_start: profile.avatar_color_start,
                avatar_color_end: profile.avatar_color_end,
            },
            content: ApiMessageContent::Text(content),
        })
    }

    async fn delete_message(&self, message_id: String) -> RenderableResult<()> {
        self.client.delete_message(&message_id).await?;
        Ok(())
    }

    async fn get_call(&self, channel_id: String) -> RenderableResult<Option<CallState>> {
        let members = self.client.get_call_members(&channel_id).await?;
        if members.is_empty() {
            return Ok(None);
        }
        let member_ids: Vec<String> = members.iter().map(|m| m.user_id.clone()).collect();
        let profiles = self
            .users
            .get_users(member_ids.clone())
            .await
            .map_err(|_| RenderableError::NetworkError)?;
        let participants = members
            .iter()
            .zip(profiles.into_iter())
            .map(|(m, profile)| CallParticipant {
                profile,
                tracks: CallTrackState {
                    audio: !m.muted,
                    video: false,
                    screen: false,
                },
            })
            .collect();
        Ok(Some(CallState { participants }))
    }

    async fn start_call(&self, channel_id: String) -> RenderableResult<()> {
        self.client.start_call(&channel_id, None).await?;
        Ok(())
    }

    async fn create_call_token(&self, channel_id: String) -> RenderableResult<CallTokenInfo> {
        let resp = self
            .client
            .create_call_token(&channel_id, false, false)
            .await?;
        Ok(CallTokenInfo {
            session_id: resp.id,
            token: resp.token,
            server_address: resp.server_address,
            call_id: resp.call_id,
        })
    }

    async fn update_voice_state(
        &self,
        channel_id: String,
        muted: Option<bool>,
        deafened: Option<bool>,
    ) -> RenderableResult<()> {
        self.client
            .update_voice_state(&channel_id, muted, deafened)
            .await?;
        Ok(())
    }

    async fn get_contacts(&self) -> RenderableResult<Vec<Contact>> {
        let contacts = self.client.get_contacts().await?;
        let mut result = Vec::with_capacity(contacts.len());
        for c in contacts {
            let profile = self
                .users
                .get_user(&c.id)
                .await
                .map_err(|_| RenderableError::NetworkError)?;
            let status = map_relationship(c.relationship);
            result.push(Contact { profile, status });
        }
        Ok(result)
    }

    async fn add_contact(&self, username: String) -> RenderableResult<Contact> {
        self.client.add_contact_username(&username).await?;
        // TODO: fix
        let contacts = self.client.get_contacts().await?;
        for c in contacts {
            let profile = self
                .users
                .get_user(&c.id)
                .await
                .map_err(|_| RenderableError::NetworkError)?;
            if profile.username == username {
                let status = map_relationship(c.relationship);
                return Ok(Contact { profile, status });
            }
        }
        Err(RenderableError::UnknownError(
            "Contact not found after sending request".into(),
        ))
    }

    async fn remove_contact(&self, user_id: String) -> RenderableResult<()> {
        self.client.remove_contact(&user_id).await?;
        Ok(())
    }

    async fn accept_contact(&self, user_id: String) -> RenderableResult<Contact> {
        self.client.add_contact(&user_id).await?;
        let contacts = self.client.get_contacts().await?;
        for c in contacts {
            if c.id == user_id {
                let profile = self
                    .users
                    .get_user(&c.id)
                    .await
                    .map_err(|_| RenderableError::NetworkError)?;
                let status = map_relationship(c.relationship);
                return Ok(Contact { profile, status });
            }
        }
        Err(RenderableError::UnknownError(
            "Contact not found after accepting request".into(),
        ))
    }

    async fn block_contact(&self, user_id: String) -> RenderableResult<()> {
        self.client.block_contact(&user_id).await?;
        Ok(())
    }

    async fn unblock_contact(&self, user_id: String) -> RenderableResult<Contact> {
        let c = self.client.unblock_contact(&user_id).await?;
        let profile = self
            .users
            .get_user(&c.id)
            .await
            .map_err(|_| RenderableError::NetworkError)?;
        Ok(Contact {
            profile,
            status: map_relationship(c.relationship),
        })
    }
}
