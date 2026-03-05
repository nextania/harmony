use std::sync::Arc;

use async_trait::async_trait;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, aead::OsRng};
use harmony_api::{AddContactStage, ClientOptions, EncryptionHint, Event, HarmonyClient, RelationshipState};
use reqwest::Client;
use rkyv::{Archive, Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};

use crate::{
    MessageAuthor,
    api::{
        ApiClient, ApiMessage, ApiMessageContent, CallParticipant, CallState, CallTokenInfo, CallTrackState, Channel, Contact, ContactAction, ContactStatus, CurrentUser, UserManager, UserProfile, UserStatus, channel_manager::ChannelManager, crypto::PersistentEncryption, keystore::Keystore, placeholder_profile
    },
    errors::{RenderableError, RenderableResult},
};

#[derive(Archive, Serialize, Deserialize)]
pub struct GroupChannelMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct LiveApiClient {
    client: HarmonyClient,
    keystore: Arc<Mutex<Keystore>>,
    user_id: String,
    users: Arc<UserManager>,
    channels: Arc<ChannelManager>,
}

impl LiveApiClient {
    pub async fn connect(
        url: &str,
        token: &str,
    ) -> Result<(Arc<dyn ApiClient>, mpsc::UnboundedReceiver<Event>), RenderableError> {
        let (client, recv) = HarmonyClient::new(ClientOptions::new(url, token)).await?;
        let current = client.get_current_user().await?;
        let keystore = if let Some(ref encrypted_keys) = current.encrypted_keys {
            // FIXME: decrypt with password-derived key, then deserialize
            Keystore::from_bytes(encrypted_keys).unwrap_or_default()
        } else {
            let ks = Keystore::new();
            let _ = client
                .set_key_package(ks.to_bytes()) // TODO: encrypt with password-derived key
                .await;
            ks
        };
        let users = UserManager::new(Client::new(), url, token);
        let channels = ChannelManager::new(client.clone());
        let live = Self {
            client,
            keystore: Arc::new(Mutex::new(keystore)),
            user_id: current.id.clone(),
            users,
            channels,
        };

        Ok((Arc::new(live), recv))
    }

    async fn sync_keystore(&self) {
        let ks = self.keystore.lock().await;
        let _ = self.client.set_key_package(ks.to_bytes()).await;
    }

    // TODO: key cache
    async fn derive_key_for_contact(&self, contact_id: &str) -> RenderableResult<Option<[u8; 32]>> {
        let contacts = self.client.get_contacts().await?;
        let contact =
            contacts
                .iter()
                .find(|c| c.id == contact_id)
                .ok_or(RenderableError::UnknownError(
                    "Failed to find contact".to_string(),
                ))?;
        let (peer_pk, their_ct) = match &contact.state {
            RelationshipState::Established {
                public_key,
                encapsulated,
            } => (public_key, encapsulated),
            _ => return Ok(None),
        };
        let ks = self.keystore.lock().await;
        let enc = ks
            .get_encryption(contact_id)
            .ok_or(RenderableError::CryptoError(
                "Failed to get encryption for contact".to_string(),
            ))?;
        let ss_1 = enc.decapsulate(their_ct);
        let ss_2 = ks
            .get_outgoing_ss(contact_id)
            .ok_or(RenderableError::CryptoError(
                "Failed to get own shared secret".to_string(),
            ))?;
        // Sort the two ML-KEM shared secrets lexicographically so both peers feed them into
        // HKDF in the same order, regardless of who was the requester and who was the acceptor.
        let (ss_a, ss_b) = if ss_1 <= ss_2 {
            (ss_1, ss_2)
        } else {
            (ss_2, ss_1)
        };
        let key = enc.derive_channel_key(peer_pk, &ss_a, &ss_b);
        Ok(Some(key))
    }

    pub async fn decrypt_content(
        &self,
        content: &[u8],
        channel_id: &str,
    ) -> RenderableResult<String> {
        if content.is_empty() {
            return Err(RenderableError::UnknownError("Empty encrypted message payload".to_string()));
        }
        let channel = self.channels.get_channel(channel_id).await?;
        match channel {
            harmony_api::Channel::GroupChannel { encryption_hint, .. } => {
                if matches!(encryption_hint, EncryptionHint::Mls) {
                    todo!()
                } else {
                    let ks = self.keystore.lock().await;
                    let Some(key) = ks.get_group_key(channel_id) else {
                        return Err(RenderableError::CryptoError(
                            "No group key available for channel".to_string(),
                        ));
                    };
                    match PersistentEncryption::decrypt_with_key(&key, content) {
                        Ok(plaintext) => return Ok(String::from_utf8_lossy(&plaintext).into_owned()),
                        Err(e) => return Err(RenderableError::CryptoError(e.to_string())),
                    }
                }
            },
            harmony_api::Channel::PrivateChannel { initiator_id, target_id, .. } => {
                let peer = if initiator_id == self.user_id {
                    target_id
                } else {
                    initiator_id
                };
                let Some(key) = self.derive_key_for_contact(&peer).await? else {
                    return Err(RenderableError::CryptoError(
                        "Failed to derive key for contact".to_string(),
                    ))
                };
                match PersistentEncryption::decrypt_with_key(&key, content) {
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
            harmony_api::Channel::GroupChannel { encryption_hint, .. } => {
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
            },
            harmony_api::Channel::PrivateChannel { initiator_id, target_id, .. } => {
                let peer = if initiator_id == self.user_id {
                    target_id
                } else {
                    initiator_id
                };
                let Some(key) = self.derive_key_for_contact(&peer).await? else {
                    return Err(RenderableError::CryptoError(
                        "Failed to derive key for contact".to_string(),
                    ))
                };
                return Ok(PersistentEncryption::encrypt_with_key(
                    &key,
                    plaintext.as_bytes(),
                ));
            }
        }
    }

    async fn map_message(&self, msg: &harmony_api::Message) -> RenderableResult<ApiMessage> {
        let text = self.decrypt_content(&msg.content, &msg.channel_id).await?;
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
}

fn map_relationship(r: &harmony_api::RelationshipState) -> ContactStatus {
    match r {
        RelationshipState::Established { .. } => ContactStatus::Established,
        RelationshipState::Blocked => ContactStatus::Blocked,
        RelationshipState::Requested { .. } => ContactStatus::Requested,
        RelationshipState::PendingKeyExchange { .. } => ContactStatus::PendingKeyExchange,
        RelationshipState::None => ContactStatus::None,
    }
}

#[async_trait]
impl ApiClient for LiveApiClient {
    async fn get_user_profile(&self, user_id: &str) -> RenderableResult<UserProfile> {
        self.users
            .get_user(user_id)
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

    async fn get_messages(&self, channel_id: &str) -> RenderableResult<Vec<ApiMessage>> {
        let messages = self
            .client
            .get_messages(channel_id, Some(50), Some(true), None, None)
            .await?;

        let mut result = Vec::with_capacity(messages.len());
        for msg in &messages {
            result.push(self.map_message(msg).await?);
        }
        Ok(result)
    }

    async fn send_message(
        &self,
        channel_id: &str,
        content: &str,
    ) -> RenderableResult<ApiMessage> {
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

    async fn edit_message(
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

    async fn delete_message(&self, message_id: &str) -> RenderableResult<()> {
        self.client.delete_message(message_id).await?;
        Ok(())
    }

    async fn get_call(&self, channel_id: &str) -> RenderableResult<Option<CallState>> {
        let members = self.client.get_call_members(channel_id).await?;
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

    async fn start_call(&self, channel_id: &str) -> RenderableResult<()> {
        self.client.start_call(channel_id, None).await?;
        Ok(())
    }

    async fn create_call_token(&self, channel_id: &str) -> RenderableResult<CallTokenInfo> {
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

    async fn update_voice_state(
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

    async fn get_contacts(&self) -> RenderableResult<Vec<Contact>> {
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

    async fn add_contact(&self, action: ContactAction) -> RenderableResult<Contact> {
        let new_state = match action {
            ContactAction::Request { username } => {
                let mut ks = self.keystore.lock().await;
                let (public_key, private_key) = ks.generate();
                let result = self.client.add_contact(AddContactStage::Request {
                    username: username.clone(),
                    public_key,
                }).await?;
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
                    RelationshipState::PendingKeyExchange {
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
                self.client.add_contact(AddContactStage::Accept {
                    user_id,
                    public_key: our_pk,
                    encapsulated: ct,
                }).await?
            }
            ContactAction::Finalize { user_id } => {
                // We are the original requester, the acceptor has responded.
                // Look up the acceptor's pk from our PendingKeyExchange state.
                let contacts = self.client.get_contacts().await?;
                let contact = contacts
                    .iter()
                    .find(|c| c.id == user_id)
                    .ok_or_else(|| RenderableError::UnknownError("Contact not found".into()))?;

                let acceptor_pk = match &contact.state {
                    RelationshipState::PendingKeyExchange {
                        public_key: Some(pk),
                        ..
                    } => pk.clone(),
                    _ => {
                        return Err(RenderableError::UnknownError(
                            "Cannot finalize: not in PendingKeyExchange state".into(),
                        ));
                    }
                };
                // Encapsulate to the acceptor's ML-KEM key and persist the shared secret so
                // it can be used for symmetric channel-key derivation later.
                let (ct, ss) = PersistentEncryption::encapsulate_to(&acceptor_pk);
                let mut ks = self.keystore.lock().await;
                ks.store_outgoing_ss(&user_id, &ss);
                let our_pk = ks.get_encryption(&user_id).map(|e| e.public_key()).ok_or(
                    RenderableError::CryptoError("Failed to get own public key for contact".into()),
                )?;
                self.client.add_contact(AddContactStage::Finalize {
                    user_id,
                    public_key: our_pk,
                    encapsulated: ct,
                }).await?
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

    async fn remove_contact(&self, user_id: &str) -> RenderableResult<()> {
        self.client.remove_contact(user_id).await?;
        Ok(())
    }

    async fn block_contact(&self, user_id: &str) -> RenderableResult<()> {
        self.client.block_contact(user_id).await?;
        Ok(())
    }

    async fn unblock_contact(&self, user_id: &str) -> RenderableResult<Contact> {
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

    async fn create_group_channel(&self, name: Option<&str>, description: Option<&str>) -> RenderableResult<Channel> {
        let metadata = GroupChannelMetadata {
            name: name.map(|s| s.to_string()),
            description: description.map(|s| s.to_string()),
        };
        let metadata_bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&metadata)
            .expect("serialization should not fail")
            .into_vec();
        let gen_key = ChaCha20Poly1305::generate_key(OsRng);
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
            participants: vec![self.users.get_user(&self.user_id).await.unwrap_or_else(|_| placeholder_profile(&self.user_id))],
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

    async fn join_group(
        &self,
        invite_code: &str,
        group_key: &[u8],
    ) -> RenderableResult<()> {
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
