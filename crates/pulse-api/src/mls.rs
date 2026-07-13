use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use hkdf::Hkdf;
use openmls::framing::MlsMessageBodyIn;
use openmls::prelude::*;
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use openmls_traits::OpenMlsProvider;
use pulse_types::{MEDIA_FRAME_HEADER_LEN, MediaHeader, decode_media_header, encode_media_header};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use tls_codec::{Deserialize as TlsDeserialize, Serialize as TlsSerialize};

use crate::events::{CallMember, CallMemberState};

/// Errors from client-side MLS state management and media frame protection.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MlsError {
    #[error("MLS group not initialized")]
    NoGroup,

    #[error("no active media epoch (MLS group not ready)")]
    NoActiveEpoch,

    #[error("malformed CBOR credential binding")]
    MalformedBinding,

    #[error("credential binding signature has wrong length")]
    BindingSignatureLength,

    #[error("invalid credential binding")]
    InvalidBinding,

    #[error("credential for user {user_id} is bound to call {bound_call}, not {call_id}")]
    CredentialWrongCall {
        user_id: String,
        bound_call: String,
        call_id: String,
    },

    #[error("credential for user {0} endorses a different leaf signature key")]
    CredentialWrongLeafKey(String),

    #[error("credential carries an invalid identity key")]
    InvalidIdentityKey,

    #[error("identity signature on credential for user {0} is invalid")]
    InvalidIdentitySignature(String),

    #[error("commit adds a member that fails authentication: {0}")]
    CommitAddUnauthenticated(Box<MlsError>),

    #[error("welcomed into a group containing an inauthentic member: {0}")]
    WelcomeInauthenticMember(Box<MlsError>),

    #[error("MlsCommit without welcome and no group")]
    CommitWithoutGroup,

    #[error("failed to generate signature key pair")]
    SignatureKeygen,

    #[error("failed to store signature keys")]
    SignatureStore,

    #[error("expected a protocol message for proposal")]
    ExpectedProtocolMessageProposal,

    #[error("expected a protocol message for commit")]
    ExpectedProtocolMessageCommit,

    #[error("expected a proposal message, got something else")]
    ExpectedProposalMessage,

    #[error("expected a StagedCommitMessage from commit data")]
    ExpectedStagedCommit,

    #[error("expected a Welcome message")]
    ExpectedWelcome,

    #[error("{operation}: {source}")]
    OpenMls {
        operation: &'static str,
        #[source]
        source: crate::error::SourceError,
    },

    #[error("encrypted media frame too short")]
    FrameTooShort,

    #[error("malformed media header")]
    MalformedHeader,

    #[error("media frame epoch {frame} does not match active epoch {active}")]
    EpochMismatch { frame: u64, active: u64 },

    #[error("replayed or reordered media frame (seq {seq} <= {last})")]
    ReplayedFrame { seq: u64, last: u64 },

    #[error("failed to encrypt media payload")]
    Encrypt,

    #[error("failed to decrypt media payload")]
    Decrypt,

    #[error("invalid media base secret length")]
    BaseSecretLength,
}

impl MlsError {
    fn op(operation: &'static str, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        MlsError::OpenMls {
            operation,
            source: std::sync::Arc::new(source),
        }
    }
}

type Result<T> = std::result::Result<T, MlsError>;

const CIPHERSUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519;
const MEDIA_LABEL: &str = "pulse-media";
const MEDIA_KEY_LEN: usize = 32;
const MEDIA_NONCE_LEN: usize = 12;
const AEAD_TAG_LEN: usize = 16;

const BINDING_HEADER: &[u8; 4] = b"HMC1";
const BINDING_SIG_DOMAIN: &[u8] = b"harmony-mls-credential-v1";

/// Resolver from a user id to their pinned account identity key.
pub type IdentityKeyResolver = Arc<dyn Fn(&str) -> Option<[u8; 32]> + Send + Sync>;

/// The local user's account identity for authenticated calls: the Ed25519
/// signing seed that signs our MLS leaf credential, plus the trust store used
/// to verify every other member's credential.
#[derive(Clone)]
pub struct MlsIdentity {
    pub user_id: String,
    pub signing_seed: [u8; 32],
    pub trusted_keys: IdentityKeyResolver,
}

impl std::fmt::Debug for MlsIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MlsIdentity")
            .field("user_id", &self.user_id)
            .finish_non_exhaustive()
    }
}

/// The contents of an identity-bound MLS credential.
#[serde_as]
#[derive(Deserialize, Serialize)]
struct CredentialBinding {
    user_id: String,
    session_id: String,
    call_id: String,
    identity_pk: [u8; 32],
    leaf_signature_key: Vec<u8>,
    #[serde_as(as = "[_; 64]")]
    signature: [u8; 64],
}

impl CredentialBinding {
    fn signed_payload(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            BINDING_SIG_DOMAIN.len()
                + 4
                + 8 * 4
                + self.user_id.len()
                + self.session_id.len()
                + self.call_id.len()
                + 32
                + self.leaf_signature_key.len(),
        );
        out.extend_from_slice(BINDING_SIG_DOMAIN);
        out.extend_from_slice(BINDING_HEADER);
        for field in [
            self.user_id.as_bytes(),
            self.session_id.as_bytes(),
            self.call_id.as_bytes(),
            self.leaf_signature_key.as_slice(),
        ] {
            out.extend_from_slice(&(field.len() as u64).to_le_bytes());
            out.extend_from_slice(field);
        }
        out.extend_from_slice(&self.identity_pk);
        out
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::from(*BINDING_HEADER);
        ciborium::into_writer(&self, &mut out)
            .expect("CBOR serialization into a Vec is infallible");
        out
    }

    fn decode(bytes: &[u8]) -> Result<Option<Self>> {
        let Some(payload) = bytes.strip_prefix(BINDING_HEADER.as_slice()) else {
            return Ok(None);
        };
        let wire: Self = ciborium::from_reader(payload).map_err(|_| MlsError::MalformedBinding)?;
        let signature: [u8; 64] = wire
            .signature
            .try_into()
            .map_err(|_| MlsError::BindingSignatureLength)?;
        Ok(Some(CredentialBinding {
            user_id: wire.user_id,
            session_id: wire.session_id,
            call_id: wire.call_id,
            identity_pk: wire.identity_pk,
            leaf_signature_key: wire.leaf_signature_key,
            signature,
        }))
    }
}

fn authenticate_member(
    call_id: &str,
    identity: &MlsIdentity,
    credential: &Credential,
    leaf_sig_key: &[u8],
) -> Result<CallMember> {
    let content = credential.serialized_content();
    let Some(binding) = CredentialBinding::decode(content)? else {
        return Err(MlsError::InvalidBinding);
    };
    if binding.call_id != call_id {
        return Err(MlsError::CredentialWrongCall {
            user_id: binding.user_id,
            bound_call: binding.call_id,
            call_id: call_id.to_string(),
        });
    }
    if binding.leaf_signature_key != leaf_sig_key {
        return Err(MlsError::CredentialWrongLeafKey(binding.user_id));
    }
    let verifying_key =
        VerifyingKey::from_bytes(&binding.identity_pk).map_err(|_| MlsError::InvalidIdentityKey)?;
    verifying_key
        .verify(
            &binding.signed_payload(),
            &Signature::from_bytes(&binding.signature),
        )
        .map_err(|_| MlsError::InvalidIdentitySignature(binding.user_id.clone()))?;

    let pinned = (identity.trusted_keys)(&binding.user_id);
    let state = match pinned {
        Some(key) if key == binding.identity_pk => CallMemberState::Verified,
        Some(_) => CallMemberState::Warning,
        None => CallMemberState::Unverified,
    };
    Ok(CallMember {
        session_id: binding.session_id,
        user_id: binding.user_id,
        state,
    })
}

/// Client-side MLS state management and media frame protection.
///
/// Handles key package generation, group initialization, commit creation,
/// and commit application. Driven internally by the `PulseClient` event loop.
pub(crate) struct MlsClient {
    provider: OpenMlsRustCrypto,
    signer: SignatureKeyPair,
    credential_with_key: CredentialWithKey,
    group: Option<MlsGroup>,
    pending_commit: Option<Vec<u8>>,
    active_epoch: Option<(u64, [u8; MEDIA_KEY_LEN])>,
    staged_secrets: HashMap<u64, [u8; MEDIA_KEY_LEN]>,
    // track name -> next send sequence
    send_seqs: HashMap<String, u64>,
    // (sender session id, track name) -> (epoch, highest accepted sequence)
    recv_seqs: HashMap<(String, String), (u64, u64)>,
    call_id: String,
    session_id: String,
    identity: MlsIdentity,
}

impl MlsClient {
    /// Create a new MLS client identity with a fresh key pair.
    pub fn new(session_id: &str, call_id: &str, identity: MlsIdentity) -> Result<Self> {
        let provider = OpenMlsRustCrypto::default();
        let signer = SignatureKeyPair::new(CIPHERSUITE.signature_algorithm())
            .map_err(|_| MlsError::SignatureKeygen)?;
        signer
            .store(provider.storage())
            .map_err(|_| MlsError::SignatureStore)?;

        let signing_key = SigningKey::from_bytes(&identity.signing_seed);
        let mut binding = CredentialBinding {
            user_id: identity.user_id.clone(),
            session_id: session_id.to_string(),
            call_id: call_id.to_string(),
            identity_pk: signing_key.verifying_key().to_bytes(),
            leaf_signature_key: signer.public().to_vec(),
            signature: [0u8; 64],
        };
        binding.signature = signing_key.sign(&binding.signed_payload()).to_bytes();
        let credential_bytes = binding.encode();
        let credential = BasicCredential::new(credential_bytes);

        let credential_with_key = CredentialWithKey {
            credential: credential.into(),
            signature_key: signer.public().into(),
        };

        Ok(Self {
            provider,
            signer,
            credential_with_key,
            group: None,
            pending_commit: None,
            active_epoch: None,
            staged_secrets: HashMap::new(),
            send_seqs: HashMap::new(),
            recv_seqs: HashMap::new(),
            call_id: call_id.to_string(),
            session_id: session_id.to_string(),
            identity: identity.clone(),
        })
    }

    /// Authenticate one member's credential against its leaf signature key.
    pub fn roster(&self) -> Vec<CallMember> {
        let Some(group) = self.group.as_ref() else {
            return Vec::new();
        };
        group
            .members()
            .filter_map(|member| {
                match authenticate_member(
                    &self.call_id,
                    &self.identity,
                    &member.credential,
                    &member.signature_key,
                ) {
                    Ok(m) => Some(m),
                    Err(e) => {
                        tracing::warn!("roster member failed authentication: {e}");
                        None
                    }
                }
            })
            .collect()
    }

    /// Generate a fresh KeyPackage and return its TLS-serialized bytes.
    ///
    /// This is sent to the server in the `Join` message's `key_package` field.
    pub fn serialized_key_package(&self) -> Result<Vec<u8>> {
        let bundle = KeyPackage::builder()
            .build(
                CIPHERSUITE,
                &self.provider,
                &self.signer,
                self.credential_with_key.clone(),
            )
            .map_err(|e| MlsError::op("Failed to build KeyPackage", e))?;

        let bytes = bundle
            .key_package()
            .tls_serialize_detached()
            .map_err(|e| MlsError::op("Failed to serialize KeyPackage", e))?;
        Ok(bytes)
    }

    /// Initialize a new MLS group as the first member.
    pub fn initialize_group(
        &mut self,
        external_sender_credential: &[u8],
        external_sender_signature_key: &[u8],
    ) -> Result<()> {
        if self.group.is_some() {
            tracing::warn!("Re-initializing MLS group, discarding previous group state");
        }

        let server_credential = BasicCredential::new(external_sender_credential.to_vec());
        let server_key: SignaturePublicKey = external_sender_signature_key.into();

        let external_sender = ExternalSender::new(server_key, Credential::from(server_credential));

        let group_config = MlsGroupCreateConfig::builder()
            .ciphersuite(CIPHERSUITE)
            .with_group_context_extensions(
                Extensions::single(Extension::ExternalSenders(vec![external_sender]))
                    .map_err(|e| MlsError::op("Failed to set group context extensions", e))?,
            )
            .use_ratchet_tree_extension(true)
            .build();

        let group = MlsGroup::new_with_group_id(
            &self.provider,
            &self.signer,
            &group_config,
            GroupId::from_slice(self.call_id.as_bytes()),
            self.credential_with_key.clone(),
        )
        .map_err(|e| MlsError::op("Failed to create MLS group", e))?;

        self.group = Some(group);
        self.pending_commit = None;
        self.recv_seqs.clear();
        self.staged_secrets.clear();

        let secret = self.export_base_secret()?;
        self.active_epoch = Some((0, secret));
        tracing::info!("MLS group initialized as first member");
        Ok(())
    }

    /// Create a commit encompassing the given proposals.
    pub fn create_commit(
        &mut self,
        proposals: &[Vec<u8>],
    ) -> Result<(Vec<u8>, u64, Option<Vec<u8>>)> {
        let group = self.group.as_mut().ok_or(MlsError::NoGroup)?;

        let mut verified_proposals = Vec::with_capacity(proposals.len());
        for p in proposals {
            let mls_message_in = MlsMessageIn::tls_deserialize(&mut p.as_slice())
                .map_err(|e| MlsError::op("Failed to deserialize proposal MlsMessageIn", e))?;
            let protocol_message = mls_message_in
                .try_into_protocol_message()
                .map_err(|_| MlsError::ExpectedProtocolMessageProposal)?;
            let processed = group
                .process_message(&self.provider, protocol_message)
                .map_err(|e| MlsError::op("Failed to process proposal message", e))?;

            let proposal = match processed.into_content() {
                ProcessedMessageContent::ProposalMessage(p) => p.proposal().clone(),
                _ => return Err(MlsError::ExpectedProposalMessage),
            };
            match &proposal {
                Proposal::Add(add) => {
                    let leaf = add.key_package().leaf_node();
                    match authenticate_member(
                        &self.call_id,
                        &self.identity,
                        leaf.credential(),
                        leaf.signature_key().as_slice(),
                    ) {
                        Ok(member) => {
                            tracing::debug!(
                                session_id = member.session_id,
                                user_id = member.user_id,
                                state = ?member.state,
                                "accepting add proposal"
                            );
                            verified_proposals.push(proposal);
                        }
                        Err(e) => {
                            tracing::warn!("rejecting add proposal: {e:#}");
                        }
                    }
                }
                Proposal::Remove(_) => verified_proposals.push(proposal),
                other => {
                    tracing::warn!(
                        "rejecting unexpected proposal type from server: {:?}",
                        other.proposal_type()
                    );
                }
            }
        }
        let proposals = verified_proposals;

        let bundle = group
            .commit_builder()
            .consume_proposal_store(false)
            .add_proposals(proposals)
            .load_psks(self.provider.storage())
            .map_err(|e| MlsError::op("Failed to load PSKs", e))?
            .build(
                self.provider.rand(),
                self.provider.crypto(),
                &self.signer,
                |_| true,
            )
            .map_err(|e| MlsError::op("Failed to build commit", e))?
            .stage_commit(&self.provider)
            .map_err(|e| MlsError::op("Failed to stage commit", e))?;

        let commit_data = bundle
            .commit()
            .tls_serialize_detached()
            .map_err(|e| MlsError::op("Failed to serialize commit", e))?;

        let welcome_data = bundle
            .to_welcome_msg()
            .map(|msg| {
                msg.to_bytes()
                    .map_err(|e| MlsError::op("Failed to serialize welcome", e))
            })
            .transpose()?;

        let epoch = group.epoch().as_u64();
        self.pending_commit = Some(commit_data.clone());

        tracing::debug!(epoch, "Created MLS commit");
        Ok((commit_data, epoch, welcome_data))
    }

    /// Apply the server-broadcast commit to advance the group epoch.
    pub fn apply_commit(&mut self, commit_data: &[u8], commit_epoch: u64) -> Result<()> {
        let group = self.group.as_mut().ok_or(MlsError::NoGroup)?;
        if commit_data == self.pending_commit.as_deref().unwrap_or(&[]) {
            // our own commit
            group
                .merge_pending_commit(&self.provider)
                .map_err(|e| MlsError::op("Failed to merge pending commit", e))?;
            self.pending_commit = None;
            tracing::debug!(epoch = group.epoch().as_u64(), "Applied own MLS commit");
        } else {
            // foreign commit
            let mls_message_in = MlsMessageIn::tls_deserialize(&mut &commit_data[..])
                .map_err(|e| MlsError::op("Failed to deserialize commit MlsMessageIn", e))?;
            let protocol_message = mls_message_in
                .try_into_protocol_message()
                .map_err(|_| MlsError::ExpectedProtocolMessageCommit)?;

            let processed = group
                .process_message(&self.provider, protocol_message)
                .map_err(|e| MlsError::op("Failed to process commit message", e))?;

            match processed.into_content() {
                ProcessedMessageContent::StagedCommitMessage(staged_commit) => {
                    for queued_add in staged_commit.add_proposals() {
                        let leaf = queued_add.add_proposal().key_package().leaf_node();
                        authenticate_member(
                            &self.call_id,
                            &self.identity,
                            leaf.credential(),
                            leaf.signature_key().as_slice(),
                        )
                        .map_err(|e| MlsError::CommitAddUnauthenticated(Box::new(e)))?;
                    }
                    group
                        .merge_staged_commit(&self.provider, *staged_commit)
                        .map_err(|e| MlsError::op("Failed to merge staged commit", e))?;
                }
                _ => return Err(MlsError::ExpectedStagedCommit),
            }

            self.pending_commit = None;
            tracing::debug!(epoch = group.epoch().as_u64(), "Applied MLS commit");
        }
        let secret = self.export_base_secret()?;
        self.staged_secrets.insert(commit_epoch + 1, secret);
        Ok(())
    }

    /// Join an existing MLS group from a welcome message.
    pub fn join_from_welcome(&mut self, welcome_data: &[u8], commit_epoch: u64) -> Result<()> {
        if self.group.is_some() {
            tracing::warn!("Joining MLS group from welcome, discarding previous group state");
            self.group = None;
        }

        let mls_message_in = MlsMessageIn::tls_deserialize(&mut &welcome_data[..])
            .map_err(|e| MlsError::op("Failed to deserialize welcome MlsMessageIn", e))?;
        let welcome = match mls_message_in.extract() {
            MlsMessageBodyIn::Welcome(w) => w,
            _ => return Err(MlsError::ExpectedWelcome),
        };

        let join_config = MlsGroupJoinConfig::builder().build();

        let group = StagedWelcome::new_from_welcome(&self.provider, &join_config, welcome, None)
            .map_err(|e| MlsError::op("Failed to stage welcome", e))?
            .into_group(&self.provider)
            .map_err(|e| MlsError::op("Failed to create group from welcome", e))?;

        for member in group.members() {
            authenticate_member(
                &self.call_id,
                &self.identity,
                &member.credential,
                &member.signature_key,
            )
            .map_err(|e| MlsError::WelcomeInauthenticMember(Box::new(e)))?;
        }

        tracing::info!(
            epoch = group.epoch().as_u64(),
            "Joined MLS group from welcome"
        );
        self.group = Some(group);
        self.pending_commit = None;
        self.recv_seqs.clear();
        self.staged_secrets.clear();

        self.active_epoch = None;
        let secret = self.export_base_secret()?;
        self.staged_secrets.insert(commit_epoch + 1, secret);
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

    /// Whether media can currently be encrypted (an epoch is active).
    pub fn media_ready(&self) -> bool {
        self.active_epoch.is_some()
    }

    /// Activate the base secret exported at the latest commit for `epoch`.
    ///
    /// All members receive `EpochReady` once every member has acked the
    /// commit; from this point only frames sealed under `epoch` are accepted.
    pub fn on_epoch_ready(&mut self, epoch: u64) {
        match self.staged_secrets.remove(&epoch) {
            Some(secret) => self.active_epoch = Some((epoch, secret)),
            None => tracing::warn!(epoch, "EpochReady without a staged base secret"),
        }
        self.staged_secrets.retain(|&e, _| e > epoch);
    }

    /// Encrypt one media frame for the track named `track_name`.
    pub fn seal_media(
        &mut self,
        track_name: &str,
        capture_ts_us: u64,
        plaintext: &[u8],
    ) -> Result<Vec<u8>> {
        let (epoch, base) = self.active_epoch.ok_or(MlsError::NoActiveEpoch)?;

        let seq = self.send_seqs.entry(track_name.to_string()).or_insert(0);
        let header = MediaHeader {
            epoch,
            sequence: *seq,
            capture_ts_us,
        };
        *seq += 1;

        let header_bytes = encode_media_header(&header);
        let key = derive_media_key(&base, &self.session_id, track_name);
        let cipher = ChaCha20Poly1305::new(&key.into());
        let aad = media_aad(&header_bytes, &self.session_id, track_name);

        let ciphertext = cipher
            .encrypt(
                &nonce_for_sequence(header.sequence),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| MlsError::Encrypt)?;

        let mut out = Vec::with_capacity(MEDIA_FRAME_HEADER_LEN + ciphertext.len());
        out.extend_from_slice(&header_bytes);
        out.extend_from_slice(&ciphertext);
        Ok(out)
    }

    /// Decrypt one media wire frame (`header || ciphertext`) from `sender_id`'s
    /// track named `track_name`.
    pub fn open_media(
        &mut self,
        sender_id: &str,
        track_name: &str,
        frame: &[u8],
    ) -> Result<(MediaHeader, Vec<u8>)> {
        if frame.len() < MEDIA_FRAME_HEADER_LEN + AEAD_TAG_LEN {
            return Err(MlsError::FrameTooShort);
        }
        let header = decode_media_header(frame).ok_or(MlsError::MalformedHeader)?;
        let header_bytes = &frame[..MEDIA_FRAME_HEADER_LEN];
        let ciphertext = &frame[MEDIA_FRAME_HEADER_LEN..];

        let (epoch, base) = self.active_epoch.ok_or(MlsError::NoActiveEpoch)?;
        if header.epoch != epoch {
            return Err(MlsError::EpochMismatch {
                frame: header.epoch,
                active: epoch,
            });
        }

        let seq_key = (sender_id.to_string(), track_name.to_string());
        if let Some(&(seen_epoch, last_seq)) = self.recv_seqs.get(&seq_key)
            && seen_epoch == epoch
            && header.sequence <= last_seq
        {
            return Err(MlsError::ReplayedFrame {
                seq: header.sequence,
                last: last_seq,
            });
        }

        let key = derive_media_key(&base, sender_id, track_name);
        let cipher = ChaCha20Poly1305::new(&key.into());
        let aad = media_aad(header_bytes, sender_id, track_name);

        let plaintext = cipher
            .decrypt(
                &nonce_for_sequence(header.sequence),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| MlsError::Decrypt)?;

        self.recv_seqs.insert(seq_key, (epoch, header.sequence));
        Ok((header, plaintext))
    }

    fn export_base_secret(&self) -> Result<[u8; MEDIA_KEY_LEN]> {
        let group = self.group.as_ref().ok_or(MlsError::NoGroup)?;
        let key_bytes = group
            .export_secret(
                self.provider.crypto(),
                MEDIA_LABEL,
                self.call_id.as_bytes(),
                MEDIA_KEY_LEN,
            )
            .map_err(|e| MlsError::op("Failed to export media base secret", e))?;
        key_bytes.try_into().map_err(|_| MlsError::BaseSecretLength)
    }
}

fn derive_media_key(
    base: &[u8; MEDIA_KEY_LEN],
    sender_id: &str,
    track_name: &str,
) -> [u8; MEDIA_KEY_LEN] {
    let hk = Hkdf::<Sha256>::from_prk(base).expect("base secret is a valid PRK length");
    let mut info = Vec::with_capacity(4 + sender_id.len() + track_name.len());
    info.extend_from_slice(&(sender_id.len() as u32).to_le_bytes());
    info.extend_from_slice(sender_id.as_bytes());
    info.extend_from_slice(track_name.as_bytes());
    let mut key = [0u8; MEDIA_KEY_LEN];
    hk.expand(&info, &mut key)
        .expect("HKDF output length is valid");
    key
}

fn media_aad(header_bytes: &[u8], sender_id: &str, track_name: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(header_bytes.len() + 4 + sender_id.len() + track_name.len());
    aad.extend_from_slice(header_bytes);
    aad.extend_from_slice(&(sender_id.len() as u32).to_le_bytes());
    aad.extend_from_slice(sender_id.as_bytes());
    aad.extend_from_slice(track_name.as_bytes());
    aad
}

fn nonce_for_sequence(sequence: u64) -> Nonce {
    let mut nonce = [0u8; MEDIA_NONCE_LEN];
    nonce[4..].copy_from_slice(&sequence.to_le_bytes());
    nonce.into()
}
