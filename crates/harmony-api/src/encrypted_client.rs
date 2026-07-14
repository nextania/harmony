use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use argon2::Argon2;
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use chacha20poly1305::{
    Key, KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate},
};
use tokio::sync::Mutex;
use zeroize::Zeroizing;

use crate::{
    Result,
    channel_manager::ChannelManager,
    client::HarmonyClient,
    crypto::{CryptoError, GROUP_METADATA_AAD, PersistentEncryption, message_aad},
    error::HarmonyError,
    events::Event,
    keystore::Keystore,
    models::{
        AddContactResponse, AddContactStage, Channel, EncryptionHint, Message, RelationshipState,
        UnifiedPublicKey,
    },
};

fn missing_key(msg: impl Into<String>) -> HarmonyError {
    HarmonyError::Crypto(CryptoError::MissingKey(msg.into()))
}

fn key_derivation(msg: impl Into<String>) -> HarmonyError {
    HarmonyError::Crypto(CryptoError::KeyDerivation(msg.into()))
}

const MAX_KEYSTORE_SYNC_RETRIES: u32 = 5;

fn encrypt_keystore(cipher: &XChaCha20Poly1305, ks: &Keystore) -> Vec<u8> {
    let nonce = XNonce::generate();
    let ciphertext = cipher
        .encrypt(&nonce, ks.to_bytes().as_ref())
        .expect("XChaCha20-Poly1305 encryption should not fail");
    [nonce.as_slice(), ciphertext.as_slice()].concat()
}

fn decrypt_keystore(cipher: &XChaCha20Poly1305, blob: &[u8]) -> Result<Keystore> {
    if blob.len() < 24 {
        return Err(CryptoError::InvalidKeystore("stored blob too short".into()).into());
    }
    let nonce: &[u8; 24] = &blob[..24].try_into().unwrap();
    let ciphertext = &blob[24..];
    let decrypted = cipher
        .decrypt(nonce.into(), ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)?;
    Keystore::from_bytes(&decrypted)
}

#[derive(Debug, Clone)]
pub enum ContactAction {
    Request {
        user_id: String,
    },
    Accept {
        user_id: String,
    },
    Finalize {
        user_id: String,
        public_key: UnifiedPublicKey,
        encapsulated: [u8; 1088],
    },
    HandleEstablished {
        user_id: String,
        public_key: UnifiedPublicKey,
        encapsulated: [u8; 1088],
        key_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum AddContactOutcome {
    Response(AddContactResponse),
    Established { user_id: String },
}

/// Derive key B from the account's `encrypted_key`and the user's password.
pub fn derive_key_b(encrypted_key: &str, password: &str) -> Result<Zeroizing<[u8; 32]>> {
    let encrypted_key_bytes = BASE64_URL_SAFE_NO_PAD
        .decode(encrypted_key)
        .map_err(|e| key_derivation(format!("failed to decode encrypted keys: {e}")))?;
    if encrypted_key_bytes.len() != 88 {
        return Err(key_derivation("invalid encrypted keys length"));
    }
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        // FIXME: the browser side uses some really weird defaults
        argon2::Params::new(1024, 1, 1, None).unwrap(),
    );
    let salt = &encrypted_key_bytes[..16];
    let mut password_key_a = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, password_key_a.as_mut())
        .map_err(|e| key_derivation(format!("failed to derive key from password: {e}")))?;
    let cipher = XChaCha20Poly1305::new((&*password_key_a).into());
    let nonce: &[u8; 24] = &encrypted_key_bytes[16..40].try_into().unwrap();
    let ciphertext = &encrypted_key_bytes[40..];
    let decrypted = Zeroizing::new(
        cipher
            .decrypt(nonce.into(), ciphertext)
            .map_err(|_| CryptoError::DecryptionFailed)?,
    );
    if decrypted.len() != 32 {
        return Err(key_derivation("invalid decrypted key B length"));
    }
    let mut key_b = Zeroizing::new([0u8; 32]);
    key_b.copy_from_slice(&decrypted);
    Ok(key_b)
}

fn key_b_cipher(key_b: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key_b.into())
}

/// End-to-end-encryption layer over [`HarmonyClient`].
#[derive(Clone)]
pub struct EncryptedClient {
    client: HarmonyClient,
    channels: Arc<ChannelManager>,
    keystore: Arc<Mutex<Keystore>>,
    generation: Arc<AtomicU64>,
    user_id: String,
    key_b: Arc<Zeroizing<[u8; 32]>>,
}

impl EncryptedClient {
    /// Wrap the [`HarmonyClient`] and initialize the keystore.
    pub async fn connect(
        client: HarmonyClient,
        encrypted_key: String,
        password: String,
    ) -> Result<Arc<Self>> {
        let key_b = derive_key_b(&encrypted_key, &password)?;

        drop(password);
        drop(encrypted_key);

        let current = client.get_current_user().await?;
        let user_id = current.id.clone();

        let cipher = key_b_cipher(&key_b);
        let (keystore, generation) = if let Some(encrypted_keys) = current.encrypted_keys {
            (
                decrypt_keystore(&cipher, &encrypted_keys)?,
                current.keystore_generation,
            )
        } else {
            let ks = Keystore::new();
            let combined = encrypt_keystore(&cipher, &ks);
            let generation = client.set_key_package(combined, 0).await?;
            (ks, generation)
        };

        let channels = Arc::new(ChannelManager::new(client.clone()));
        let this = Arc::new(Self {
            client,
            channels,
            keystore: Arc::new(Mutex::new(keystore)),
            generation: Arc::new(AtomicU64::new(generation)),
            user_id,
            key_b: Arc::new(key_b),
        });
        Ok(this)
    }

    pub fn client(&self) -> &HarmonyClient {
        &self.client
    }

    pub fn channels(&self) -> &Arc<ChannelManager> {
        &self.channels
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// Re-encrypt the keystore under key B and upload it to the server with a
    /// compare-and-swap on the last-known generation.
    pub async fn sync_keystore(&self) -> Result<()> {
        let cipher = key_b_cipher(&self.key_b);
        for _ in 0..MAX_KEYSTORE_SYNC_RETRIES {
            let expected = self.generation.load(Ordering::SeqCst);
            let combined = {
                let ks = self.keystore.lock().await;
                encrypt_keystore(&cipher, &ks)
            };
            match self.client.set_key_package(combined, expected).await {
                Ok(new_generation) => {
                    self.generation.store(new_generation, Ordering::SeqCst);
                    return Ok(());
                }
                Err(HarmonyError::Api(crate::error::ApiError::KeystoreConflict)) => {
                    self.reconcile_keystore(&cipher).await?;
                }
                Err(e) => return Err(e),
            }
        }
        Err(HarmonyError::KeystoreSyncFailed {
            attempts: MAX_KEYSTORE_SYNC_RETRIES,
        })
    }

    /// Fetch the current server keystore, merge it into the local one, and adopt
    /// the server's generation so the next upload's compare-and-swap can succeed.
    async fn reconcile_keystore(&self, cipher: &XChaCha20Poly1305) -> Result<()> {
        let current = self.client.get_current_user().await?;
        match current.encrypted_keys {
            Some(blob) => {
                let remote = decrypt_keystore(cipher, &blob)?;
                let mut ks = self.keystore.lock().await;
                ks.merge(&remote);
                self.generation
                    .store(current.keystore_generation, Ordering::SeqCst);
            }
            None => {
                // Server has no keystore (unexpected on conflict, but recover by
                // re-attempting as a first-ever upload rather than looping).
                self.generation.store(0, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub async fn decrypt_content(&self, msg: &Message) -> Result<Vec<u8>> {
        if msg.content.is_empty() {
            return Err(CryptoError::InvalidCiphertext.into());
        }
        let aad = message_aad(&msg.channel_id, &msg.author_id);
        let channel = self.channels.get_channel(&msg.channel_id).await?;
        match channel {
            Channel::GroupChannel {
                encryption_hint, ..
            } => {
                if matches!(encryption_hint, EncryptionHint::Mls) {
                    todo!()
                } else {
                    let ks = self.keystore.lock().await;
                    let Some(key) = ks.get_group_key(&msg.channel_id) else {
                        return Err(missing_key("no group key available for channel"));
                    };
                    Ok(PersistentEncryption::decrypt_with_key(
                        &key,
                        &msg.content,
                        &aad,
                    )?)
                }
            }
            Channel::PrivateChannel { .. } => {
                let Some(key_id) = &msg.key_id else {
                    return Err(missing_key("missing key ID for private message"));
                };
                let ks = self.keystore.lock().await;
                let Some(key) = ks.get_direct_key(key_id) else {
                    return Err(missing_key("no direct key stored for contact"));
                };
                Ok(PersistentEncryption::decrypt_with_key(
                    &key,
                    &msg.content,
                    &aad,
                )?)
            }
        }
    }

    pub async fn encrypt_content(&self, channel_id: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let aad = message_aad(channel_id, &self.user_id);
        let channel = self.channels.get_channel(channel_id).await?;
        match channel {
            Channel::GroupChannel {
                encryption_hint, ..
            } => {
                if matches!(encryption_hint, EncryptionHint::Mls) {
                    todo!()
                } else {
                    let ks = self.keystore.lock().await;
                    let Some(key) = ks.get_group_key(channel_id) else {
                        return Err(missing_key("no group key available for channel"));
                    };
                    Ok(PersistentEncryption::encrypt_with_key(
                        &key, plaintext, &aad,
                    ))
                }
            }
            Channel::PrivateChannel { last_key_id, .. } => {
                let ks = self.keystore.lock().await;
                let Some(key) = ks.get_direct_key(&last_key_id) else {
                    return Err(missing_key("no direct key stored for contact"));
                };
                Ok(PersistentEncryption::encrypt_with_key(
                    &key, plaintext, &aad,
                ))
            }
        }
    }

    fn pin_peer_identity(
        &self,
        ks: &mut Keystore,
        user_id: &str,
        pk: &UnifiedPublicKey,
    ) -> Result<()> {
        ks.pin_identity_key(user_id, pk.ed25519)
    }

    pub async fn identity_seed(&self) -> Zeroizing<[u8; 32]> {
        let ks = self.keystore.lock().await;
        Zeroizing::new(ks.identity_seed())
    }

    pub async fn identity_verifying_key(&self) -> [u8; 32] {
        let ks = self.keystore.lock().await;
        ks.identity_verifying_key()
    }

    pub async fn pinned_identity_key(&self, user_id: &str) -> Option<[u8; 32]> {
        let ks = self.keystore.lock().await;
        ks.get_pinned_identity_key(user_id)
    }

    pub async fn identity_key_snapshot(&self) -> std::collections::HashMap<String, [u8; 32]> {
        let ks = self.keystore.lock().await;
        let mut map = ks.pinned_identity_keys();
        map.insert(self.user_id.clone(), ks.identity_verifying_key());
        map
    }

    pub async fn safety_number(&self, contact_id: &str) -> Option<String> {
        let ks = self.keystore.lock().await;
        let ours = ks.identity_verifying_key();
        let theirs = ks.get_pinned_identity_key(contact_id)?;
        Some(crate::crypto::safety_number(
            &self.user_id,
            &ours,
            contact_id,
            &theirs,
        ))
    }

    pub async fn add_contact(&self, action: ContactAction) -> Result<AddContactOutcome> {
        let response = match action {
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
                    .ok_or(HarmonyError::ContactNotFound)?;
                let requester_pk = match &contact.state {
                    RelationshipState::Requested {
                        public_key: Some(pk),
                        ..
                    } => pk.clone(),
                    _ => {
                        return Err(HarmonyError::RequesterPublicKeyUnavailable);
                    }
                };
                let mut ks = self.keystore.lock().await;
                self.pin_peer_identity(&mut ks, &user_id, &requester_pk)?;
                let (our_pk, our_sk) = ks.generate();
                // Encapsulate to the requester's ML-KEM key and persist the shared secret so
                // it can be used for symmetric channel-key derivation later.
                let (ct, ss) = PersistentEncryption::encapsulate_to(&requester_pk.hybrid)?;
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
            ContactAction::Finalize {
                user_id,
                public_key: acceptor_pk,
                encapsulated,
            } => {
                // We are the original requester, the acceptor has responded.

                // decapsulate the acceptor's response to get the shared secret
                let mut ks = self.keystore.lock().await;
                self.pin_peer_identity(&mut ks, &user_id, &acceptor_pk)?;
                let enc = ks
                    .get_encryption(&user_id)
                    .ok_or_else(|| missing_key("no negotiation key stored for contact"))?;
                let ss1 = enc.decapsulate(&encapsulated)?;
                // encapsulate back to the acceptor to get the second shared secret
                let (ct, ss2) = PersistentEncryption::encapsulate_to(&acceptor_pk.hybrid)?;
                ks.store_outgoing_ss(&user_id, &ss2);

                let our_pk = UnifiedPublicKey {
                    hybrid: enc.public_key(),
                    ed25519: ks.identity_verifying_key(),
                };
                let result = self
                    .client
                    .add_contact(AddContactStage::Finalize {
                        user_id: user_id.clone(),
                        public_key: our_pk,
                        encapsulated: ct,
                    })
                    .await?;
                let RelationshipState::Established { ref key_id, .. } = result.state else {
                    return Err(HarmonyError::UnexpectedRelationshipState);
                };
                let key = enc.derive_channel_key(
                    &self.user_id,
                    ks.identity_verifying_key(),
                    &user_id,
                    &acceptor_pk,
                    &ss1,
                    &ss2,
                )?;
                ks.store_direct_key(key_id, key);
                result
            }
            ContactAction::HandleEstablished {
                user_id,
                public_key: requester_pk,
                encapsulated,
                key_id,
            } => {
                {
                    let mut ks = self.keystore.lock().await;
                    self.pin_peer_identity(&mut ks, &user_id, &requester_pk)?;
                    let enc = ks
                        .get_encryption(&user_id)
                        .ok_or_else(|| missing_key("no negotiation key stored for contact"))?;
                    let ss2 = enc.decapsulate(&encapsulated)?;
                    let ss1 = ks.get_outgoing_ss(&user_id).ok_or_else(|| {
                        missing_key("no outgoing shared secret stored for contact")
                    })?;
                    let key = enc.derive_channel_key(
                        &self.user_id,
                        ks.identity_verifying_key(),
                        &user_id,
                        &requester_pk,
                        &ss1,
                        &ss2,
                    )?;
                    ks.store_direct_key(&key_id, key);
                }
                self.sync_keystore().await?;
                return Ok(AddContactOutcome::Established { user_id });
            }
        };

        // sync keystore to server after any state change
        self.sync_keystore().await?;
        Ok(AddContactOutcome::Response(response))
    }

    pub async fn create_group_channel(&self, metadata_plaintext: &[u8]) -> Result<Channel> {
        let gen_key = Key::generate();
        let mut key = [0u8; 32];
        key.copy_from_slice(&gen_key);
        let encrypted_metadata =
            PersistentEncryption::encrypt_with_key(&key, metadata_plaintext, GROUP_METADATA_AAD);
        let channel = self
            .client
            .create_group_channel(encrypted_metadata, EncryptionHint::Persistent)
            .await?;
        let channel_id = channel.id().to_string();
        {
            let mut ks = self.keystore.lock().await;
            ks.store_group_key(&channel_id, &key);
        }
        self.sync_keystore().await?;
        Ok(channel)
    }

    pub async fn create_group_invite(&self, channel_id: &str) -> Result<String> {
        let invite = self
            .client
            .create_invite(channel_id, Some(1), None, None)
            .await?;
        Ok(invite.code)
    }

    pub async fn join_group(&self, invite_code: &str, group_key: &[u8]) -> Result<String> {
        let (_pending, channel_id) = self.client.accept_invite(invite_code).await?;
        if group_key.len() != 32 {
            return Err(HarmonyError::InvalidGroupKeyLength(group_key.len()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(group_key);
        {
            let mut ks = self.keystore.lock().await;
            ks.store_group_key(&channel_id, &key);
        }
        self.sync_keystore().await?;
        Ok(channel_id)
    }

    pub async fn get_group_key(&self, channel_id: &str) -> Option<Vec<u8>> {
        let ks = self.keystore.lock().await;
        ks.get_group_key(channel_id).map(|k| k.to_vec())
    }

    pub async fn handle_event(&self, event: &Event) -> Result<Option<AddContactOutcome>> {
        match event {
            Event::ContactStateChanged { user_id, state } => match state {
                RelationshipState::PendingKeyExchange {
                    public_key: Some(public_key),
                    encapsulated: Some(encapsulated),
                } => {
                    // We are the requester and the acceptor has responded.
                    let outcome = self
                        .add_contact(ContactAction::Finalize {
                            user_id: user_id.clone(),
                            public_key: public_key.clone(),
                            encapsulated: *encapsulated,
                        })
                        .await?;
                    Ok(Some(outcome))
                }
                RelationshipState::Established {
                    public_key,
                    encapsulated,
                    key_id,
                } => {
                    let outcome = self
                        .add_contact(ContactAction::HandleEstablished {
                            user_id: user_id.clone(),
                            public_key: public_key.clone(),
                            encapsulated: *encapsulated,
                            key_id: key_id.clone(),
                        })
                        .await?;
                    Ok(Some(outcome))
                }
                _ => Ok(None),
            },
            Event::ChannelUpdated(e) => {
                self.channels.update(e.channel.clone());
                Ok(None)
            }
            Event::ChannelDeleted(e) => {
                self.channels.invalidate(&e.channel_id);
                Ok(None)
            }
            Event::MemberJoined(e) => {
                self.channels.invalidate(&e.channel_id);
                Ok(None)
            }
            Event::MemberLeft(e) => {
                self.channels.invalidate(&e.channel_id);
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}
