use std::collections::HashMap;

use getrandom::{
    SysRng,
    rand_core::{Rng, UnwrapErr},
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::{
    Result,
    crypto::{CryptoError, HYBRID_SECRET_KEY_BYTES, PersistentEncryption, UnifiedPublicKey},
    error::HarmonyError,
};

const KEYSTORE_HEADER: &[u8; 4] = b"HKS\0";
const KEYSTORE_VERSION: u16 = 1;

#[serde_as]
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct ContactPrivateKey {
    // secret key bytes
    #[serde_as(as = "[_; HYBRID_SECRET_KEY_BYTES]")]
    hybrid_pk: [u8; HYBRID_SECRET_KEY_BYTES],
    // outgoing ML-KEM shared secrets
    #[serde(default)]
    outgoing_ss: Option<[u8; 32]>,
}

impl std::fmt::Debug for ContactPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContactPrivateKey(<redacted>)")
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Keystore {
    negotiation_keys: HashMap<String, ContactPrivateKey>,
    // key ID -> symmetric ChaCha20-Poly1305 key for private channels
    direct_keys: HashMap<String, [u8; 32]>,
    // group channel ID -> symmetric ChaCha20-Poly1305 key
    group_keys: HashMap<String, [u8; 32]>,
    // Ed25519 identity signing seed
    identity_seed: [u8; 32],
    // contact user ID -> pinned Ed25519 identity verifying key
    pinned_identity_keys: HashMap<String, [u8; 32]>,
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Keystore")
            .field("negotiation_keys", &self.negotiation_keys.len())
            .field("direct_keys", &self.direct_keys.len())
            .field("group_keys", &self.group_keys.len())
            .field("pinned_identity_keys", &self.pinned_identity_keys.len())
            .finish()
    }
}

impl Drop for Keystore {
    fn drop(&mut self) {
        for key in self.direct_keys.values_mut() {
            key.zeroize();
        }
        for key in self.group_keys.values_mut() {
            key.zeroize();
        }
        self.identity_seed.as_mut().zeroize();
    }
}

impl Keystore {
    pub fn new() -> Self {
        let mut ks = Self::default();
        UnwrapErr(SysRng).fill_bytes(&mut ks.identity_seed);
        ks
    }

    pub fn generate(&self) -> (UnifiedPublicKey, ContactPrivateKey) {
        let enc = PersistentEncryption::generate();
        let pk = UnifiedPublicKey {
            hybrid: enc.public_key(),
            ed25519: self.identity_verifying_key(),
        };
        let contact_key = ContactPrivateKey {
            hybrid_pk: enc.secret_key_bytes(),
            outgoing_ss: None,
        };
        (pk, contact_key)
    }

    pub fn identity_seed(&self) -> [u8; 32] {
        self.identity_seed
    }

    pub fn identity_verifying_key(&self) -> [u8; 32] {
        crate::crypto::identity_verifying_key(&self.identity_seed)
    }

    pub fn pin_identity_key(&mut self, user_id: &str, key: [u8; 32]) -> Result<()> {
        match self.pinned_identity_keys.get(user_id) {
            Some(pinned) if *pinned != key => Err(HarmonyError::Crypto(
                CryptoError::IdentityKeyMismatch(user_id.to_string()),
            )),
            Some(_) => Ok(()),
            None => {
                self.pinned_identity_keys.insert(user_id.to_string(), key);
                Ok(())
            }
        }
    }

    pub fn get_pinned_identity_key(&self, user_id: &str) -> Option<[u8; 32]> {
        self.pinned_identity_keys.get(user_id).copied()
    }

    pub fn pinned_identity_keys(&self) -> HashMap<String, [u8; 32]> {
        self.pinned_identity_keys.clone()
    }

    pub fn store_direct_key(&mut self, key_id: &str, key: [u8; 32]) {
        self.direct_keys.insert(key_id.to_string(), key);
    }
    pub fn get_direct_key(&self, key_id: &str) -> Option<[u8; 32]> {
        self.direct_keys.get(key_id).copied()
    }

    pub fn store_contact_key(&mut self, contact_id: &str, contact_key: ContactPrivateKey) {
        self.negotiation_keys
            .insert(contact_id.to_string(), contact_key);
    }

    pub fn get_encryption(&self, contact_id: &str) -> Option<PersistentEncryption> {
        self.negotiation_keys
            .get(contact_id)
            .map(|contact_key| PersistentEncryption::from_secret_bytes(contact_key.hybrid_pk))
    }

    pub fn has_contact(&self, contact_id: &str) -> bool {
        self.negotiation_keys.contains_key(contact_id)
    }

    pub fn store_outgoing_ss(&mut self, contact_id: &str, ss: &[u8; 32]) {
        if let Some(contact_key) = self.negotiation_keys.get_mut(contact_id)
            && contact_key.outgoing_ss.is_none()
        {
            contact_key.outgoing_ss = Some(*ss);
        }
    }

    pub fn get_outgoing_ss(&self, contact_id: &str) -> Option<[u8; 32]> {
        self.negotiation_keys
            .get(contact_id)
            .and_then(|contact_key| contact_key.outgoing_ss)
    }

    pub fn store_group_key(&mut self, channel_id: &str, key: &[u8; 32]) {
        self.group_keys.insert(channel_id.to_string(), *key);
    }

    pub fn get_group_key(&self, channel_id: &str) -> Option<[u8; 32]> {
        self.group_keys.get(channel_id).copied()
    }

    /// Union-merge another keystore into this one.
    pub fn merge(&mut self, other: &Keystore) {
        for (contact_id, remote) in &other.negotiation_keys {
            match self.negotiation_keys.get_mut(contact_id) {
                Some(local) => {
                    if local.outgoing_ss.is_none() {
                        local.outgoing_ss = remote.outgoing_ss;
                    }
                }
                None => {
                    self.negotiation_keys
                        .insert(contact_id.clone(), remote.clone());
                }
            }
        }
        for (key_id, key) in &other.direct_keys {
            self.direct_keys.entry(key_id.clone()).or_insert(*key);
        }
        for (channel_id, key) in &other.group_keys {
            self.group_keys.entry(channel_id.clone()).or_insert(*key);
        }
        if self.identity_seed != other.identity_seed {
            tracing::warn!("identity seed conflict during merge; adopting the server copy");
            self.identity_seed = other.identity_seed;
        }

        for (user_id, key) in &other.pinned_identity_keys {
            match self.pinned_identity_keys.get(user_id) {
                Some(local) if local != key => {
                    tracing::warn!(user_id, "conflicting pinned identity keys during merge");
                }
                Some(_) => {}
                None => {
                    self.pinned_identity_keys.insert(user_id.clone(), *key);
                }
            }
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 6 || &bytes[..4] != KEYSTORE_HEADER {
            return Err(HarmonyError::Crypto(CryptoError::InvalidKeystore(
                "invalid blob header".to_string(),
            )));
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != KEYSTORE_VERSION {
            return Err(HarmonyError::Crypto(CryptoError::InvalidKeystore(format!(
                "unsupported version {version}"
            ))));
        }
        ciborium::from_reader(&bytes[6..]).map_err(|e| {
            HarmonyError::Crypto(CryptoError::InvalidKeystore(format!(
                "failed to decode: {e}"
            )))
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(KEYSTORE_HEADER);
        out.extend_from_slice(&KEYSTORE_VERSION.to_le_bytes());
        ciborium::into_writer(self, &mut out).expect("keystore serialization should not fail");
        out
    }
}
