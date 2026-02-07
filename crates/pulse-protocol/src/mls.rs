use anyhow::{Context, Result, bail};
use chacha20poly1305::aead::rand_core::RngCore;
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use dashmap::DashMap;
use openmls::framing::MlsMessageBodyIn;
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use std::time::{Duration, Instant};
use tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519;
const MEDIA_LABEL: &str = "pulse-media";
const MEDIA_KEY_LEN: usize = 32;
const MEDIA_NONCE_LEN: usize = 12;
const MEDIA_KEY_GRACE_PERIOD: Duration = Duration::from_secs(5);

/// Client-side MLS state management.
///
/// Handles key package generation, group initialization, commit creation,
/// and commit application. Driven internally by the `PulseClient` event loop.
pub struct MlsClient {
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    credential_with_key: CredentialWithKey,
    group: Option<MlsGroup>,
    has_pending_commit: bool,
    media_keys: DashMap<String, [u8; MEDIA_KEY_LEN]>,
    previous_media_keys: DashMap<String, [u8; MEDIA_KEY_LEN]>,
    previous_keys_expiry: Option<Instant>,
    media_epoch: Option<u64>,
}

impl MlsClient {
    /// Create a new MLS client identity with a fresh key pair.
    ///
    /// `client_id` is an opaque identifier (e.g. the session ID) embedded in the credential.
    pub fn new(client_id: &str) -> Result<Self> {
        let provider = OpenMlsRustCrypto::default();
        let credential = BasicCredential::new(client_id.as_bytes().to_vec());
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm())
            .map_err(|_| anyhow::anyhow!("Failed to generate signature key pair"))?;
        signer
            .store(provider.storage())
            .map_err(|_| anyhow::anyhow!("Failed to store signature keys"))?;

        let credential_with_key = CredentialWithKey {
            credential: credential.into(),
            signature_key: signer.public().into(),
        };

        Ok(Self {
            provider,
            signer,
            credential_with_key,
            group: None,
            has_pending_commit: false,
            media_keys: DashMap::new(),
            previous_media_keys: DashMap::new(),
            previous_keys_expiry: None,
            media_epoch: None,
        })
    }

    /// Generate a fresh KeyPackage and return its TLS-serialized bytes.
    ///
    /// This is sent to the server in the `Connect` message's `key_package` field.
    pub fn serialized_key_package(&self) -> Result<Vec<u8>> {
        let bundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential_with_key.clone(),
            )
            .context("Failed to build KeyPackage")?;

        let bytes = bundle
            .key_package()
            .tls_serialize_detached()
            .context("Failed to serialize KeyPackage")?;
        Ok(bytes)
    }

    /// Initialize a new MLS group as the first member.
    ///
    /// Called when the server sends `InitializeGroup` with its external sender identity.
    /// Configures the server as an authorized external sender so it can create Add/Remove proposals.
    pub fn initialize_group(
        &mut self,
        external_sender_credential: &[u8],
        external_sender_signature_key: &[u8],
    ) -> Result<()> {
        if self.group.is_some() {
            bail!("MLS group already initialized");
        }

        let server_credential = BasicCredential::new(external_sender_credential.to_vec());
        let server_key: SignaturePublicKey = external_sender_signature_key.into();

        let external_sender = ExternalSender::new(server_key, Credential::from(server_credential));

        let group_config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .with_group_context_extensions(Extensions::single(Extension::ExternalSenders(vec![
                external_sender,
            ])))
            .context("Failed to set group context extensions")?
            .build();

        let group = MlsGroup::new(
            &self.provider,
            &self.signer,
            &group_config,
            self.credential_with_key.clone(),
        )
        .context("Failed to create MLS group")?;

        self.group = Some(group);
        self.has_pending_commit = false;
        self.media_keys.clear();
        self.previous_media_keys.clear();
        self.previous_keys_expiry = None;
        self.media_epoch = Some(0);
        tracing::info!("MLS group initialized as first member");
        Ok(())
    }

    /// Create a commit encompassing the given proposals.
    ///
    /// Called when the server sends `MlsProposals`. Uses `commit_builder` to create the commit
    /// without advancing the group epoch — the group enters `PendingCommit` state.
    /// The commit is only applied when the server broadcasts it back via `MlsCommit`.
    ///
    /// Returns `(commit_data, epoch, Option<welcome_data>)` to send as `WtMessageC2S::MlsCommit`.
    pub fn create_commit(
        &mut self,
        proposals: &[Vec<u8>],
    ) -> Result<(Vec<u8>, u64, Option<Vec<u8>>)> {
        let group = self.group.as_mut().context("MLS group not initialized")?;

        for proposal_bytes in proposals {
            let mls_message_in = MlsMessageIn::tls_deserialize(&mut proposal_bytes.as_slice())
                .context("Failed to deserialize proposal MlsMessageIn")?;
            let protocol_message = mls_message_in
                .try_into_protocol_message()
                .map_err(|_| anyhow::anyhow!("Expected a protocol message for proposal"))?;
            let processed = group
                .process_message(&self.provider, protocol_message)
                .context("Failed to process proposal message")?;

            // Ensure it's actually a proposal
            match processed.into_content() {
                ProcessedMessageContent::ProposalMessage(_)
                | ProcessedMessageContent::ExternalJoinProposalMessage(_) => {}
                _ => bail!("Expected a proposal message, got something else"),
            }
        }

        let bundle = group
            .commit_builder()
            .load_psks(self.provider.storage())
            .context("Failed to load PSKs")?
            .build(
                self.provider.rand(),
                self.provider.crypto(),
                &self.signer,
                |_| true, // accept all queued proposals
            )
            .context("Failed to build commit")?
            .stage_commit(&self.provider)
            .context("Failed to stage commit")?;

        let commit_data = bundle
            .commit()
            .tls_serialize_detached()
            .context("Failed to serialize commit")?;

        let welcome_data = bundle
            .to_welcome_msg()
            .map(|msg| msg.to_bytes().context("Failed to serialize welcome"))
            .transpose()?;

        let epoch = group.epoch().as_u64();
        self.has_pending_commit = true;

        tracing::debug!(epoch, "Created MLS commit");
        Ok((commit_data, epoch, welcome_data))
    }

    /// Apply the server-broadcast commit to advance the group epoch.
    ///
    /// Called when the server sends `MlsCommit`. If we have a pending commit for this epoch,
    /// we try to merge it as our own. If the commit is from another member, we process it
    /// as a foreign commit, which implicitly discards our pending commit.
    ///
    /// For new members joining via welcome, call `join_from_welcome` instead.
    pub fn apply_commit(&mut self, commit_data: &[u8]) -> Result<()> {
        let group = self.group.as_mut().context("MLS group not initialized")?;

        let mls_message_in = MlsMessageIn::tls_deserialize(&mut commit_data.to_vec().as_slice())
            .context("Failed to deserialize commit MlsMessageIn")?;
        let protocol_message = mls_message_in
            .try_into_protocol_message()
            .map_err(|_| anyhow::anyhow!("Expected a protocol message for commit"))?;

        let processed = group
            .process_message(&self.provider, protocol_message)
            .context("Failed to process commit message")?;

        match processed.into_content() {
            ProcessedMessageContent::StagedCommitMessage(staged_commit) => {
                group
                    .merge_staged_commit(&self.provider, *staged_commit)
                    .context("Failed to merge staged commit")?;
            }
            _ => bail!("Expected a StagedCommitMessage from commit data"),
        }

        self.has_pending_commit = false;
        tracing::debug!(epoch = group.epoch().as_u64(), "Applied MLS commit");
        Ok(())
    }

    /// Join an existing MLS group from a welcome message.
    ///
    /// Called when the server sends `MlsCommit` with `welcome_data` for a member
    /// that does not yet have a group.
    pub fn join_from_welcome(&mut self, welcome_data: &[u8]) -> Result<()> {
        if self.group.is_some() {
            bail!("Already in an MLS group, cannot join from welcome");
        }

        let mls_message_in = MlsMessageIn::tls_deserialize(&mut welcome_data.to_vec().as_slice())
            .context("Failed to deserialize welcome MlsMessageIn")?;
        let welcome = match mls_message_in.extract() {
            MlsMessageBodyIn::Welcome(w) => w,
            _ => bail!("Expected a Welcome message"),
        };

        let join_config = MlsGroupJoinConfig::builder().build();

        let group = StagedWelcome::new_from_welcome(&self.provider, &join_config, welcome, None)
            .context("Failed to stage welcome")?
            .into_group(&self.provider)
            .context("Failed to create group from welcome")?;

        tracing::info!(
            epoch = group.epoch().as_u64(),
            "Joined MLS group from welcome"
        );
        self.group = Some(group);
        self.has_pending_commit = false;
        self.media_keys.clear();
        self.previous_media_keys.clear();
        self.previous_keys_expiry = None;
        self.media_epoch = Some(0);
        Ok(())
    }

    /// Return the current MLS epoch number, or 0 if no group is initialized.
    pub fn current_epoch(&self) -> u64 {
        self.group.as_ref().map(|g| g.epoch().as_u64()).unwrap_or(0)
    }

    /// Whether the client has an MLS group initialized.
    pub fn has_group(&self) -> bool {
        self.group.is_some()
    }

    /// Refresh media key cache when the server signals an epoch is ready.
    ///
    /// This clears the cached keys. New keys are derived lazily on next use.
    pub fn on_epoch_ready(&mut self, epoch: u64) {
        self.previous_media_keys = std::mem::take(&mut self.media_keys);
        self.previous_keys_expiry = Some(Instant::now() + MEDIA_KEY_GRACE_PERIOD);
        self.media_epoch = Some(epoch);
    }

    /// Encrypt media payload data for a given track using MLS exporter secret.
    ///
    /// Returns `nonce || ciphertext` where the nonce is 12 bytes.
    pub fn encrypt_media(&self, track_id: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
        let key = self.media_key_current(track_id)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));

        let mut nonce_bytes = [0u8; MEDIA_NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext,
                    aad: track_id.as_bytes(),
                },
            )
            .map_err(|_| anyhow::anyhow!("Failed to encrypt media payload"))?;

        let mut out = Vec::with_capacity(MEDIA_NONCE_LEN + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt media payload data for a given track using MLS exporter secret.
    ///
    /// Expects `nonce || ciphertext` where the nonce is 12 bytes.
    pub fn decrypt_media(&self, track_id: &str, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < MEDIA_NONCE_LEN + 16 {
            bail!("Encrypted media payload too short");
        }

        let key = self.media_key_current(track_id)?;
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));

        let (nonce_bytes, ciphertext) = data.split_at(MEDIA_NONCE_LEN);
        let nonce = Nonce::from_slice(nonce_bytes);

        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: track_id.as_bytes(),
                },
            )
            .map_err(|_| anyhow::anyhow!("Failed to decrypt media payload"));

        if let Ok(plaintext) = &plaintext {
            return Ok(plaintext.clone());
        }

        // try decrypting with previous key if within grace period
        if let Some(expiry) = self.previous_keys_expiry
            && Instant::now() < expiry
            && let Some(prev_key) = self.previous_media_keys.get(track_id)
        {
            let prev_cipher = ChaCha20Poly1305::new(Key::from_slice(prev_key.value()));
            let prev_plaintext = prev_cipher.decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: track_id.as_bytes(),
                },
            );
            if let Ok(prev_plaintext) = prev_plaintext {
                return Ok(prev_plaintext);
            }
        }
        Err(anyhow::anyhow!(
            "Failed to decrypt media payload with current or previous keys"
        ))
    }

    fn media_key_current(&self, track_id: &str) -> Result<[u8; MEDIA_KEY_LEN]> {
        if let Some(key) = self.media_keys.get(track_id) {
            return Ok(*key);
        }

        let key = self.export_media_key(track_id)?;
        self.media_keys.insert(track_id.to_string(), key);
        Ok(key)
    }

    fn export_media_key(&self, track_id: &str) -> Result<[u8; MEDIA_KEY_LEN]> {
        let group = self.group.as_ref().context("MLS group not initialized")?;

        let key_bytes = group
            .export_secret(
                self.provider.crypto(),
                MEDIA_LABEL,
                track_id.as_bytes(),
                MEDIA_KEY_LEN,
            )
            .context("Failed to export media key")?;

        let key: [u8; MEDIA_KEY_LEN] = key_bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Invalid media key length"))?;
        Ok(key)
    }
}
