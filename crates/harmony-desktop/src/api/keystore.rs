use std::collections::HashMap;

use rkyv::{Archive, Deserialize, Serialize};

use super::crypto::{HYBRID_SECRET_KEY_BYTES, PersistentEncryption, UnifiedPublicKey};

#[derive(Clone, Debug, Serialize, Deserialize, Archive)]
pub struct ContactPrivateKey {
    // secret key bytes
    hybrid_pk: [u8; HYBRID_SECRET_KEY_BYTES],
    // outgoing ML-KEM shared secrets
    outgoing_ss: Option<[u8; 32]>,
    // derived symmetric key
    final_sk: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Archive)]
pub struct Keystore {
    contact_private_keys: HashMap<String, ContactPrivateKey>,
    // group channel ID -> symmetric ChaCha20-Poly1305 key
    group_keys: HashMap<String, [u8; 32]>,
}

impl Keystore {
    pub fn new() -> Self {
        Self {
            contact_private_keys: HashMap::new(),
            group_keys: HashMap::new(),
        }
    }

    pub fn generate(&self) -> (UnifiedPublicKey, ContactPrivateKey) {
        let enc = PersistentEncryption::generate();
        let pk = enc.public_key();
        let contact_key = ContactPrivateKey {
            hybrid_pk: enc.secret_key_bytes(),
            outgoing_ss: None,
            final_sk: None,
        };
        (pk, contact_key)
    }

    pub fn store_contact_key(&mut self, contact_id: &str, contact_key: ContactPrivateKey) {
        self.contact_private_keys.insert(contact_id.to_string(), contact_key);
    }

    pub fn get_encryption(&self, contact_id: &str) -> Option<PersistentEncryption> {
        self.contact_private_keys
            .get(contact_id)
            .map(|contact_key| PersistentEncryption::from_secret_bytes(contact_key.hybrid_pk))
    }

    pub fn has_contact(&self, contact_id: &str) -> bool {
        self.contact_private_keys.contains_key(contact_id)
    }

    pub fn store_outgoing_ss(&mut self, contact_id: &str, ss: &[u8; 32]) {
        if let Some(contact_key) = self.contact_private_keys.get_mut(contact_id) {
            contact_key.outgoing_ss = Some(*ss);
        }
    }

    pub fn get_outgoing_ss(&self, contact_id: &str) -> Option<[u8; 32]> {
        self.contact_private_keys.get(contact_id).and_then(|contact_key| contact_key.outgoing_ss)
    }

    pub fn store_group_key(&mut self, channel_id: &str, key: &[u8; 32]) {
        self.group_keys.insert(channel_id.to_string(), *key);
    }

    pub fn get_group_key(&self, channel_id: &str) -> Option<[u8; 32]> {
        self.group_keys.get(channel_id).copied()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        rkyv::from_bytes::<_, rkyv::rancor::Error>(bytes).ok()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        rkyv::to_bytes::<rkyv::rancor::Error>(self)
            .expect("serialization should not fail")
            .into_vec()
    }
}

impl Default for Keystore {
    fn default() -> Self {
        Self::new()
    }
}
