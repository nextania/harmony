use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate},
};
use core_api::Session;
use harmony_types::{
    events::{
        CallMigratedEvent, UserJoinedCallEvent, UserLeftCallEvent, UserVoiceStateChangedEvent,
    },
    users::Encapsulated,
};
use tokio::sync::{Mutex, broadcast};
use zeroize::Zeroizing;

use crate::{
    Result,
    channel::{Channel, DecryptedMessage},
    channel_manager::ChannelManager,
    client::{ClientOptions, HarmonyClient},
    crypto::{CryptoError, PersistentEncryption, message_aad},
    error::HarmonyError,
    events::{ClientEvent, Event, LifecycleEvent},
    keystore::Keystore,
    models::{
        AddContactResponse, AddContactStage, ChannelData, EncryptionHint, Message,
        RelationshipState, UnifiedPublicKey,
    },
    user_manager::UserManager,
};

fn missing_key(msg: impl Into<String>) -> HarmonyError {
    HarmonyError::Crypto(CryptoError::MissingKey(msg.into()))
}

const MAX_KEYSTORE_SYNC_RETRIES: u32 = 5;

const EVENT_CHANNEL_CAPACITY: usize = 256;

fn single(event: EncryptedEvent) -> Vec<EncryptedEvent> {
    vec![event]
}

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
        encapsulated: Encapsulated,
    },
    HandleEstablished {
        user_id: String,
        public_key: UnifiedPublicKey,
        encapsulated: Encapsulated,
        key_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum AddContactOutcome {
    Response(AddContactResponse),
    Established { user_id: String },
}

/// A fully-processed event emitted by [`EncryptedClient`].
#[derive(Debug, Clone)]
pub enum EncryptedEvent {
    Lifecycle(LifecycleEvent),
    NewMessage {
        channel_id: String,
        message: DecryptedMessage,
    },
    MessageEdited {
        channel_id: String,
        message: DecryptedMessage,
    },
    MessageDeleted {
        channel_id: String,
        message_id: String,
    },
    ChannelUpdated {
        channel: Channel,
    },
    ChannelDeleted {
        channel_id: String,
    },
    MemberJoined {
        channel_id: String,
        user_id: String,
    },
    MemberLeft {
        channel_id: String,
        user_id: String,
    },
    UserJoinedCall(UserJoinedCallEvent),
    UserLeftCall(UserLeftCallEvent),
    UserVoiceStateChanged(UserVoiceStateChangedEvent),
    CallMigrated(CallMigratedEvent),
    ContactStateChanged {
        user_id: String,
        state: RelationshipState,
    },
    ContactAdded(AddContactOutcome),
}

pub(crate) struct Core {
    pub(crate) client: HarmonyClient,
    pub(crate) keystore: Mutex<Keystore>,
    pub(crate) generation: AtomicU64,
    pub(crate) user_id: String,
    session: Arc<Session>,
}

impl Core {
    /// Re-encrypt the keystore under key B and upload it to the server with a
    /// compare-and-swap on the last-known generation.
    pub(crate) async fn sync_keystore(&self) -> Result<()> {
        let cipher = self.session.cipher();
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
                self.generation.store(0, Ordering::SeqCst);
            }
        }
        Ok(())
    }

    pub(crate) async fn decrypt_content(
        &self,
        channel: &ChannelData,
        msg: &Message,
    ) -> Result<Vec<u8>> {
        if msg.content.is_empty() {
            return Err(CryptoError::InvalidCiphertext.into());
        }
        let aad = message_aad(&msg.channel_id, &msg.author_id);
        match channel {
            ChannelData::GroupChannel {
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
            ChannelData::PrivateChannel { .. } => {
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

    pub(crate) async fn encrypt_content(
        &self,
        channel: &ChannelData,
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        let aad = message_aad(channel.id(), &self.user_id);
        match channel {
            ChannelData::GroupChannel {
                encryption_hint, ..
            } => {
                if matches!(encryption_hint, EncryptionHint::Mls) {
                    todo!()
                } else {
                    let ks = self.keystore.lock().await;
                    let Some(key) = ks.get_group_key(channel.id()) else {
                        return Err(missing_key("no group key available for channel"));
                    };
                    Ok(PersistentEncryption::encrypt_with_key(
                        &key, plaintext, &aad,
                    ))
                }
            }
            ChannelData::PrivateChannel { last_key_id, .. } => {
                let ks = self.keystore.lock().await;
                let Some(key) = ks.get_direct_key(last_key_id) else {
                    return Err(missing_key("no direct key stored for contact"));
                };
                Ok(PersistentEncryption::encrypt_with_key(
                    &key, plaintext, &aad,
                ))
            }
        }
    }
}

/// End-to-end-encryption layer over [`HarmonyClient`].
#[derive(Clone)]
pub struct EncryptedClient {
    core: Arc<Core>,
    channels: Arc<ChannelManager>,
    users: Arc<UserManager>,
    events_tx: broadcast::Sender<EncryptedEvent>,
}

impl EncryptedClient {
    /// Connect to the server, initialize the keystore, and start automatically
    /// processing incoming events. Returns the client and the raw event stream
    /// for the consumer.
    pub async fn connect(
        session: Arc<Session>,
        options: ClientOptions,
    ) -> Result<(Arc<Self>, broadcast::Receiver<EncryptedEvent>)> {
        let (client, consumer_rx) = HarmonyClient::new(session.clone(), options).await?;

        let current = client.get_current_user().await?;
        let user_id = current.id.clone();

        let cipher = session.cipher();
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

        let core = Arc::new(Core {
            client,
            session: session.clone(),
            keystore: Mutex::new(keystore),
            generation: AtomicU64::new(generation),
            user_id,
        });
        let users = Arc::new(UserManager::new(core.clone(), session.clone()));
        let channels = Arc::new(ChannelManager::new(core.clone(), users.clone()));
        let (events_tx, events_rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let this = Arc::new(Self {
            core,
            channels,
            users,
            events_tx,
        });
        this.spawn_event_pump(consumer_rx);
        Ok((this, events_rx))
    }

    fn spawn_event_pump(self: &Arc<Self>, mut rx: broadcast::Receiver<ClientEvent>) {
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(client_event) => {
                        let Some(this) = weak.upgrade() else {
                            break;
                        };
                        let events = match client_event {
                            ClientEvent::Lifecycle(lifecycle) => {
                                vec![EncryptedEvent::Lifecycle(lifecycle)]
                            }
                            ClientEvent::Event(event) => match this.handle_event(event).await {
                                Ok(events) => events,
                                Err(e) => {
                                    tracing::warn!("failed to process event: {e}");
                                    continue;
                                }
                            },
                        };
                        for event in events {
                            let _ = this.events_tx.send(event);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("crypto event pump lagged; {n} events dropped");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    pub fn client(&self) -> &HarmonyClient {
        &self.core.client
    }

    pub fn channels(&self) -> &Arc<ChannelManager> {
        &self.channels
    }

    pub fn users(&self) -> &Arc<UserManager> {
        &self.users
    }

    pub fn user_id(&self) -> &str {
        &self.core.user_id
    }

    pub async fn sync_keystore(&self) -> Result<()> {
        self.core.sync_keystore().await
    }

    pub async fn encrypt_content(&self, channel_id: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let channel = self.channels.fetch(channel_id).await?;
        let channel = channel.data();
        self.core.encrypt_content(channel, plaintext).await
    }

    pub async fn decrypt_content(&self, msg: &Message) -> Result<Vec<u8>> {
        let channel = self.channels.fetch(&msg.channel_id).await?;
        let channel = channel.data();
        self.core.decrypt_content(channel, msg).await
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
        let ks = self.core.keystore.lock().await;
        Zeroizing::new(ks.identity_seed())
    }

    pub async fn identity_verifying_key(&self) -> [u8; 32] {
        let ks = self.core.keystore.lock().await;
        ks.identity_verifying_key()
    }

    pub async fn pinned_identity_key(&self, user_id: &str) -> Option<[u8; 32]> {
        let ks = self.core.keystore.lock().await;
        ks.get_pinned_identity_key(user_id)
    }

    pub async fn identity_key_snapshot(&self) -> std::collections::HashMap<String, [u8; 32]> {
        let ks = self.core.keystore.lock().await;
        let mut map = ks.pinned_identity_keys();
        map.insert(self.core.user_id.clone(), ks.identity_verifying_key());
        map
    }

    pub async fn safety_number(&self, contact_id: &str) -> Option<String> {
        let ks = self.core.keystore.lock().await;
        let ours = ks.identity_verifying_key();
        let theirs = ks.get_pinned_identity_key(contact_id)?;
        Some(crate::crypto::safety_number(
            &self.core.user_id,
            &ours,
            contact_id,
            &theirs,
        ))
    }

    pub async fn add_contact(&self, action: ContactAction) -> Result<AddContactOutcome> {
        let response = match action {
            ContactAction::Request { user_id } => {
                let mut ks = self.core.keystore.lock().await;
                let (public_key, private_key) = ks.generate();
                let result = self
                    .core
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
                let contacts = self.core.client.get_contacts().await?;
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
                let mut ks = self.core.keystore.lock().await;
                self.pin_peer_identity(&mut ks, &user_id, &requester_pk)?;
                let (our_pk, our_sk) = ks.generate();
                // Encapsulate to the requester's ML-KEM key and persist the shared secret so
                // it can be used for symmetric channel-key derivation later.
                let (ct, ss) = PersistentEncryption::encapsulate_to(&requester_pk.hybrid)?;
                ks.store_contact_key(&user_id, our_sk);
                ks.store_outgoing_ss(&user_id, &ss);
                self.core
                    .client
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
                let mut ks = self.core.keystore.lock().await;
                self.pin_peer_identity(&mut ks, &user_id, &acceptor_pk)?;
                let enc = ks
                    .get_encryption(&user_id)
                    .ok_or_else(|| missing_key("no negotiation key stored for contact"))?;
                let ss1 = enc.decapsulate(encapsulated.as_slice())?;
                // encapsulate back to the acceptor to get the second shared secret
                let (ct, ss2) = PersistentEncryption::encapsulate_to(&acceptor_pk.hybrid)?;
                ks.store_outgoing_ss(&user_id, &ss2);

                let our_pk = UnifiedPublicKey {
                    hybrid: enc.public_key(),
                    ed25519: ks.identity_verifying_key(),
                };
                let result = self
                    .core
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
                    &self.core.user_id,
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
                    let mut ks = self.core.keystore.lock().await;
                    self.pin_peer_identity(&mut ks, &user_id, &requester_pk)?;
                    let enc = ks
                        .get_encryption(&user_id)
                        .ok_or_else(|| missing_key("no negotiation key stored for contact"))?;
                    let ss2 = enc.decapsulate(encapsulated.as_slice())?;
                    let ss1 = ks.get_outgoing_ss(&user_id).ok_or_else(|| {
                        missing_key("no outgoing shared secret stored for contact")
                    })?;
                    let key = enc.derive_channel_key(
                        &self.core.user_id,
                        ks.identity_verifying_key(),
                        &user_id,
                        &requester_pk,
                        &ss1,
                        &ss2,
                    )?;
                    ks.store_direct_key(&key_id, key);
                }
                self.core.sync_keystore().await?;
                return Ok(AddContactOutcome::Established { user_id });
            }
        };

        // sync keystore to server after any state change
        self.core.sync_keystore().await?;
        Ok(AddContactOutcome::Response(response))
    }

    async fn handle_event(&self, event: Event) -> Result<Vec<EncryptedEvent>> {
        Ok(match event {
            Event::ContactStateChanged { user_id, state } => {
                let outcome = self.advance_contact_handshake(&user_id, &state).await?;
                let mut events = vec![EncryptedEvent::ContactStateChanged { user_id, state }];
                events.extend(outcome.map(EncryptedEvent::ContactAdded));
                events
            }
            Event::NewMessage(e) => {
                let channel = self.channels.fetch(&e.channel_id).await?;
                let content = channel.receive_message(&e.message).await?;
                single(EncryptedEvent::NewMessage {
                    channel_id: e.channel_id,
                    message: DecryptedMessage {
                        message: e.message,
                        content,
                    },
                })
            }
            Event::MessageEdited(e) => {
                let channel = self.channels.fetch(&e.channel_id).await?;
                let content = channel.receive_message(&e.message).await?;
                single(EncryptedEvent::MessageEdited {
                    channel_id: e.channel_id,
                    message: DecryptedMessage {
                        message: e.message,
                        content,
                    },
                })
            }
            Event::MessageDeleted(e) => {
                if let Some(channel) = self.channels.get(&e.channel_id) {
                    channel.remove_cached(&e.message_id);
                }
                single(EncryptedEvent::MessageDeleted {
                    channel_id: e.channel_id,
                    message_id: e.message_id,
                })
            }
            Event::ChannelUpdated(e) => {
                let channel = self.channels.update(e.channel);
                single(EncryptedEvent::ChannelUpdated { channel })
            }
            Event::ChannelDeleted(e) => {
                self.channels.invalidate(&e.channel_id);
                single(EncryptedEvent::ChannelDeleted {
                    channel_id: e.channel_id,
                })
            }
            Event::MemberJoined(e) => {
                self.channels.invalidate(&e.channel_id);
                single(EncryptedEvent::MemberJoined {
                    channel_id: e.channel_id,
                    user_id: e.user_id,
                })
            }
            Event::MemberLeft(e) => {
                self.channels.invalidate(&e.channel_id);
                single(EncryptedEvent::MemberLeft {
                    channel_id: e.channel_id,
                    user_id: e.user_id,
                })
            }
            Event::UserJoinedCall(e) => single(EncryptedEvent::UserJoinedCall(e)),
            Event::UserLeftCall(e) => single(EncryptedEvent::UserLeftCall(e)),
            Event::UserVoiceStateChanged(e) => single(EncryptedEvent::UserVoiceStateChanged(e)),
            Event::CallMigrated(e) => single(EncryptedEvent::CallMigrated(e)),
        })
    }

    async fn advance_contact_handshake(
        &self,
        user_id: &str,
        state: &RelationshipState,
    ) -> Result<Option<AddContactOutcome>> {
        match state {
            RelationshipState::PendingKeyExchange {
                public_key: Some(public_key),
                encapsulated: Some(encapsulated),
            } => {
                // We are the requester and the acceptor has responded.
                let outcome = self
                    .add_contact(ContactAction::Finalize {
                        user_id: user_id.to_string(),
                        public_key: public_key.clone(),
                        encapsulated: encapsulated.clone(),
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
                        user_id: user_id.to_string(),
                        public_key: public_key.clone(),
                        encapsulated: encapsulated.clone(),
                        key_id: key_id.clone(),
                    })
                    .await?;
                Ok(Some(outcome))
            }
            _ => Ok(None),
        }
    }
}
