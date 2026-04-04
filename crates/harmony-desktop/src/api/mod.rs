pub mod account;
pub mod channel_manager;
pub mod crypto;
pub mod keystore;
pub mod user_manager;

pub use user_manager::UserManager;

use harmony_api::UnifiedPublicKey;

use iced::Color;

use std::sync::Arc;

use argon2::Argon2;
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use chacha20poly1305::{AeadCore, KeyInit, XChaCha20Poly1305, aead::{Aead, OsRng}};
use harmony_api::{
    AddContactStage, ClientOptions, EncryptionHint, Event, HarmonyClient, RelationshipState,
};
use reqwest::Client;
use rkyv::{Archive, Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};

use crate::{
    MessageAuthor,
    api::{
        channel_manager::ChannelManager, crypto::PersistentEncryption,
        keystore::Keystore,
    },
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

#[derive(Debug, Clone)]
pub enum ContactAction {
    Request { user_id: String },
    Accept { user_id: String },
    Finalize { user_id: String, public_key: UnifiedPublicKey, encapsulated: Vec<u8> },
    HandleEstablished { user_id: String, public_key: UnifiedPublicKey, encapsulated: Vec<u8>, key_id: String },
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

// An ApiClient directly maps API methods to renderables
#[derive(Clone)]
pub struct ApiClient {
    client: HarmonyClient,
    keystore: Arc<Mutex<Keystore>>,
    user_id: String,
    users: Arc<UserManager>,
    channels: Arc<ChannelManager>,
    encrypted_key: String,
    password: String,
}

pub fn get_key_b(encrypted_key: &str, password: &str) -> RenderableResult<XChaCha20Poly1305> {
    let encrypted_key_bytes = BASE64_URL_SAFE_NO_PAD.decode(encrypted_key)
        .map_err(|e| RenderableError::CryptoError(format!("Failed to decode encrypted keys: {e}")))?;
    if encrypted_key_bytes.len() != 88 {
        return Err(RenderableError::CryptoError("Invalid encrypted keys length".into()));
    }
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        // FIXME: the browser side uses some really weird defaults
        argon2::Params::new(1024, 1, 1, None).unwrap(),
    );
    let salt = &encrypted_key_bytes[..16];
    let mut password_key_a = [0u8; 32];
    argon2.hash_password_into(password.as_bytes(), salt, &mut password_key_a).map_err(|e| RenderableError::CryptoError(format!("Failed to derive key from password: {e}")))?;
    let cipher = XChaCha20Poly1305::new(&password_key_a.into());
    let nonce = &encrypted_key_bytes[16..40];
    let ciphertext = &encrypted_key_bytes[40..];
    let decrypted = cipher.decrypt(nonce.into(), ciphertext)
        .map_err(|e| RenderableError::CryptoError(format!("Failed to decrypt keys: {e}")))?;
    if decrypted.len() != 32 {
        return Err(RenderableError::CryptoError("Invalid decrypted key B length".into()));
    }
    let decrypted: [u8; 32] = decrypted.try_into().map_err(|_| RenderableError::CryptoError("Failed to convert decrypted key to array".into()))?;
    Ok(XChaCha20Poly1305::new(&decrypted.into()))
}

impl ApiClient {
    pub async fn connect(
        as_url: &str,
        harmony_url: &str,
        token: &str,
        encrypted_key: &str,
        password: &str,
    ) -> Result<(Arc<ApiClient>, mpsc::UnboundedReceiver<Event>), RenderableError> {
        let (client, recv) = HarmonyClient::new(ClientOptions::new(harmony_url, token)).await?;
        let current = client.get_current_user().await?;
        let keystore = if let Some(ref encrypted_keys) = current.encrypted_keys {
            let key_b = get_key_b(encrypted_key, password)?;
            let nonce = &encrypted_keys[..24];
            let ciphertext = &encrypted_keys[24..];
            let decrypted_keys = key_b.decrypt(nonce.into(), ciphertext)
                .map_err(|e| RenderableError::CryptoError(format!("Failed to decrypt keys with key B: {e}")))?;
            Keystore::from_bytes(&decrypted_keys).unwrap_or_default()
        } else {
            let ks = Keystore::new();
            // encrypt
            let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
            let cipher = get_key_b(encrypted_key, password)?;
            let encrypted_key_a = cipher.encrypt(&nonce, ks.to_bytes().as_ref())
                .map_err(|e| RenderableError::CryptoError(format!("Failed to encrypt keys: {e}")))?;
            let combined = [nonce.as_slice(), encrypted_key_a.as_slice()].concat();
            client
                .set_key_package(combined)
                .await?;
            ks
        };
        let users = UserManager::new(Client::new(), as_url, token);
        let channels = ChannelManager::new(client.clone());
        let live = Self {
            client,
            keystore: Arc::new(Mutex::new(keystore)),
            user_id: current.id.clone(),
            users,
            channels,
            encrypted_key: encrypted_key.to_string(),
            password: password.to_string(),
        };

        Ok((Arc::new(live), recv))
    }

    async fn sync_keystore(&self) -> RenderableResult<()> {
        let ks = self.keystore.lock().await;
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let cipher = get_key_b(&self.encrypted_key, &self.password)?;
        let encrypted_key_a = cipher.encrypt(&nonce, ks.to_bytes().as_ref())
            .map_err(|e| RenderableError::CryptoError(format!("Failed to encrypt keys: {e}")))?;
        let combined = [nonce.as_slice(), encrypted_key_a.as_slice()].concat();
        self.client.set_key_package(combined).await?;
        Ok(())
    }

    pub async fn decrypt_content(&self, msg: &harmony_api::Message) -> RenderableResult<String> {
        if msg.content.is_empty() {
            return Err(RenderableError::UnknownError(
                "Empty encrypted message payload".to_string(),
            ));
        }
        let channel = self.channels.get_channel(&msg.channel_id).await?;
        match channel {
            harmony_api::Channel::GroupChannel {
                encryption_hint, ..
            } => {
                if matches!(encryption_hint, EncryptionHint::Mls) {
                    todo!()
                } else {
                    let ks = self.keystore.lock().await;
                    let Some(key) = ks.get_group_key(&msg.channel_id) else {
                        return Err(RenderableError::CryptoError(
                            "No group key available for channel".to_string(),
                        ));
                    };
                    match PersistentEncryption::decrypt_with_key(&key, &msg.content) {
                        Ok(plaintext) => {
                            return Ok(String::from_utf8_lossy(&plaintext).into_owned());
                        }
                        Err(e) => return Err(RenderableError::CryptoError(e.to_string())),
                    }
                }
            }
            harmony_api::Channel::PrivateChannel { .. } => {
                let Some(key_id) = &msg.key_id else {
                    return Err(RenderableError::CryptoError(
                        "Missing key ID for private message".to_string(),
                    ));
                };
                let ks = self.keystore.lock().await;
                let Some(key) = ks.get_direct_key(key_id) else {
                    return Err(RenderableError::CryptoError(
                        "Failed to derive key for contact".to_string(),
                    ));
                };
                match PersistentEncryption::decrypt_with_key(&key, &msg.content) {
                    Ok(plaintext) => return Ok(String::from_utf8_lossy(&plaintext).into_owned()),
                    Err(e) => return Err(RenderableError::CryptoError(e.to_string())),
                }
            }
        }
    }

    async fn encrypt_content(
        &self,
        plaintext: &str,
        channel_id: &str,
    ) -> RenderableResult<Vec<u8>> {
        let channel = self.channels.get_channel(channel_id).await?;
        match channel {
            harmony_api::Channel::GroupChannel {
                encryption_hint, ..
            } => {
                if matches!(encryption_hint, EncryptionHint::Mls) {
                    todo!()
                } else {
                    let ks = self.keystore.lock().await;
                    let Some(key) = ks.get_group_key(channel_id) else {
                        return Err(RenderableError::CryptoError(
                            "No group key available for channel".to_string(),
                        ));
                    };
                    return Ok(PersistentEncryption::encrypt_with_key(
                        &key,
                        plaintext.as_bytes(),
                    ));
                }
            }
            harmony_api::Channel::PrivateChannel { last_key_id, .. } => {
                let ks = self.keystore.lock().await;
                let Some(key) = ks.get_direct_key(&last_key_id) else {
                    return Err(RenderableError::CryptoError(
                        "Failed to derive key for contact".to_string(),
                    ));
                };
                return Ok(PersistentEncryption::encrypt_with_key(
                    &key,
                    plaintext.as_bytes(),
                ));
            }
        }
    }

    pub async fn map_message(&self, msg: &harmony_api::Message) -> RenderableResult<ApiMessage> {
        let text = self.decrypt_content(msg).await?;
        let profile = self
            .users
            .get_user(&msg.author_id)
            .await
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
            .map_err(|_| RenderableError::NetworkError)
    }

    pub async fn get_user_profile_by_username(&self, username: &str) -> RenderableResult<UserProfile> {
        self.users
            .get_user_by_username(username)
            .await
            .map_err(|_| RenderableError::NetworkError)
    }

    pub async fn get_user_profiles(&self, user_ids: Vec<String>) -> RenderableResult<Vec<UserProfile>> {
        self.users
            .get_users(user_ids.clone())
            .await
            .map_err(|_| RenderableError::NetworkError)
    }

    pub async fn get_current_user(&self) -> RenderableResult<CurrentUser> {
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

    pub async fn get_conversations(&self) -> RenderableResult<Vec<Channel>> {
        let channels = self.client.get_channels().await?;
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

    pub async fn get_messages(&self, channel_id: &str) -> RenderableResult<Vec<ApiMessage>> {
        let messages = self
            .client
            .get_messages(channel_id, Some(50), None, None, None)
            .await?;

        let mut result = Vec::with_capacity(messages.len());
        for msg in &messages {
            result.push(self.map_message(msg).await?);
        }
        Ok(result)
    }

    pub async fn send_message(&self, channel_id: &str, content: &str) -> RenderableResult<ApiMessage> {
        let encrypted = self.encrypt_content(content, channel_id).await?;
        let msg = self.client.send_message(channel_id, encrypted).await?;
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
            content: ApiMessageContent::Text(content.to_string()),
        })
    }

    pub async fn edit_message(
        &self,
        message_id: &str,
        channel_id: &str,
        content: &str,
    ) -> RenderableResult<ApiMessage> {
        let encrypted = self.encrypt_content(content, channel_id).await?;
        let msg = self.client.edit_message(message_id, encrypted).await?;
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
            content: ApiMessageContent::Text(content.to_string()),
        })
    }

    pub async fn delete_message(&self, message_id: &str) -> RenderableResult<()> {
        self.client.delete_message(message_id).await?;
        Ok(())
    }

    pub async fn get_call(&self, channel_id: &str) -> RenderableResult<Option<CallState>> {
        let members = self.client.get_call_members(channel_id).await?;
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
        self.client.start_call(channel_id, None).await?;
        Ok(())
    }

    pub async fn create_call_token(&self, channel_id: &str) -> RenderableResult<CallTokenInfo> {
        let resp = self
            .client
            .create_call_token(channel_id, false, false)
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
        self.client
            .update_voice_state(channel_id, muted, deafened)
            .await?;
        Ok(())
    }

    pub async fn get_contacts(&self) -> RenderableResult<Vec<Contact>> {
        let contacts = self.client.get_contacts().await?;
        let mut result = Vec::with_capacity(contacts.len());
        for c in contacts {
            let profile = self
                .users
                .get_user(&c.id)
                .await
                .map_err(|_| RenderableError::NetworkError)?;
            let status = map_relationship(&c.state);
            result.push(Contact { profile, status });
        }
        Ok(result)
    }

    pub async fn add_contact(&self, action: ContactAction) -> RenderableResult<Contact> {
        let new_state = match action {
            ContactAction::Request { user_id } => {
                let mut ks = self.keystore.lock().await;
                let (public_key, private_key) = ks.generate();
                let result = self
                    .client
                    .add_contact(AddContactStage::Request {
                        id: user_id.clone(),
                        public_key,
                    })
                    .await?;
                ks.store_contact_key(&result.profile.id, private_key);
                result
            }
            ContactAction::Accept { user_id } => {
                let contacts = self.client.get_contacts().await?;
                let contact = contacts
                    .iter()
                    .find(|c| c.id == user_id)
                    .ok_or_else(|| RenderableError::UnknownError("Contact not found".into()))?;
                let requester_pk = match &contact.state {
                    RelationshipState::Requested {
                        public_key: Some(pk),
                        ..
                    } => pk.clone(),
                    _ => {
                        return Err(RenderableError::UnknownError(
                            "Cannot accept: requester's public key not available".into(),
                        ));
                    }
                };
                let mut ks = self.keystore.lock().await;
                let (our_pk, our_sk) = ks.generate();
                // Encapsulate to the requester's ML-KEM key and persist the shared secret so
                // it can be used for symmetric channel-key derivation later.
                let (ct, ss) = PersistentEncryption::encapsulate_to(&requester_pk);
                ks.store_contact_key(&user_id, our_sk);
                ks.store_outgoing_ss(&user_id, &ss);
                self.client
                    .add_contact(AddContactStage::Accept {
                        user_id,
                        public_key: our_pk,
                        encapsulated: ct,
                    })
                    .await?
            }
            ContactAction::Finalize { user_id, public_key: acceptor_pk, encapsulated  } => {
                // We are the original requester, the acceptor has responded.
                
                // decapsulate the acceptor's response to get the shared secret
                let mut ks = self.keystore.lock().await;
                let enc = ks
                    .get_encryption(&user_id)
                    .ok_or(RenderableError::CryptoError(
                        "Failed to get encryption for contact".to_string(),
                    ))?;
                let ss1 = enc.decapsulate(&encapsulated);
                // encapsulate back to the acceptor to get the second shared secret
                let (ct, ss2) = PersistentEncryption::encapsulate_to(&acceptor_pk);
                ks.store_outgoing_ss(&user_id, &ss2);

                let our_pk = enc.public_key();
                let result = self
                    .client
                    .add_contact(AddContactStage::Finalize {
                        user_id,
                        public_key: our_pk,
                        encapsulated: ct,
                    })
                    .await?;
                let RelationshipState::Established { ref key_id, .. } = result.state else {
                    return Err(RenderableError::UnknownError(
                        "Expected Established relationship state after finalizing contact".into(),
                    ));
                };
                let key = enc.derive_channel_key(&acceptor_pk, &ss1, &ss2);
                ks.store_direct_key(key_id, key);
                result
            }
            ContactAction::HandleEstablished { user_id, public_key: requester_pk, encapsulated, key_id } => {
                let mut ks = self.keystore.lock().await;
                let enc = ks
                    .get_encryption(&user_id)
                    .ok_or(RenderableError::CryptoError(
                        "Failed to get encryption for contact".to_string(),
                    ))?;
                let ss2 = enc.decapsulate(&encapsulated);
                let ss1 = ks
                    .get_outgoing_ss(&user_id)
                    .ok_or(RenderableError::CryptoError(
                        "Failed to get outgoing shared secret for contact".to_string(),
                    ))?;
                let key = enc.derive_channel_key(&requester_pk, &ss1, &ss2);
                ks.store_direct_key(&key_id, key);
                drop(ks);
                self.sync_keystore().await?;
                let profile = self
                    .users
                    .get_user(&user_id)
                    .await
                    .unwrap_or_else(|_| placeholder_profile(&user_id));
                return Ok(Contact {
                    profile,
                    status: ContactStatus::Established,
                });
            }
        };

        // sync keystore to server after any state change
        self.sync_keystore().await;

        if let Ok(profile) = self.users.get_user(&new_state.profile.id).await {
            return Ok(Contact {
                profile,
                status: map_relationship(&new_state.state),
            });
        }

        Err(RenderableError::UnknownError(
            "Contact not found after operation".into(),
        ))
    }

    pub async fn remove_contact(&self, user_id: &str) -> RenderableResult<()> {
        self.client.remove_contact(user_id).await?;
        Ok(())
    }

    pub async fn block_contact(&self, user_id: &str) -> RenderableResult<()> {
        self.client.block_contact(user_id).await?;
        Ok(())
    }

    pub async fn unblock_contact(&self, user_id: &str) -> RenderableResult<Contact> {
        let c = self.client.unblock_contact(user_id).await?;
        let profile = self
            .users
            .get_user(&c.id)
            .await
            .map_err(|_| RenderableError::NetworkError)?;
        Ok(Contact {
            profile,
            status: map_relationship(&c.state),
        })
    }

    pub async fn create_private_channel(&self, user_id: &str) -> RenderableResult<Channel> {
        let api_channel = self.client.create_private_channel(user_id).await?;
        let profile = self
            .users
            .get_user(user_id)
            .await
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
        let gen_key = XChaCha20Poly1305::generate_key(OsRng);
        let mut key = [0u8; 32];
        key.copy_from_slice(&gen_key);
        let encrypted_metadata = PersistentEncryption::encrypt_with_key(&key, &metadata_bytes);
        let api_channel = self
            .client
            .create_group_channel(encrypted_metadata, EncryptionHint::Persistent)
            .await?;
        let channel_id = api_channel.id().to_string();
        {
            let mut ks = self.keystore.lock().await;
            ks.store_group_key(&channel_id, &key);
        }
        self.sync_keystore().await;
        Ok(Channel::Group {
            id: channel_id,
            name: metadata.name,
            participants: vec![
                self.users
                    .get_user(&self.user_id)
                    .await
                    .unwrap_or_else(|_| placeholder_profile(&self.user_id)),
            ],
        })
    }

    async fn get_group_key(&self, channel_id: &str) -> RenderableResult<Option<Vec<u8>>> {
        let ks = self.keystore.lock().await;
        Ok(ks.get_group_key(&channel_id).map(|k| k.to_vec()))
    }

    async fn create_group_invite(&self, channel_id: &str) -> RenderableResult<String> {
        let invite = self
            .client
            .create_invite(&channel_id, Some(1), None, None)
            .await?;
        Ok(invite.code)
    }

    async fn join_group(&self, invite_code: &str, group_key: &[u8]) -> RenderableResult<()> {
        let (_pending, channel_id) = self.client.accept_invite(&invite_code).await?;
        if group_key.len() != 32 {
            return Err(RenderableError::CryptoError(
                "Group key must be exactly 32 bytes".into(),
            ));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&group_key);
        {
            let mut ks = self.keystore.lock().await;
            ks.store_group_key(&channel_id, &key);
        }
        self.sync_keystore().await;
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
