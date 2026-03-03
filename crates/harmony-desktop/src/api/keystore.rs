use std::collections::HashMap;

use rkyv::{Archive, Deserialize, Serialize};

use super::crypto::{HYBRID_SECRET_KEY_BYTES, PersistentEncryption, UnifiedPublicKey};

#[derive(Clone, Debug, Serialize, Deserialize, Archive)]
pub struct Keystore {
    // contact user ID -> secret key bytes
    contact_private_keys: HashMap<String, [u8; HYBRID_SECRET_KEY_BYTES]>,
    // outgoing ML-KEM shared secrets
    outgoing_ss: HashMap<String, [u8; 32]>,
}

impl Keystore {
    pub fn new() -> Self {
        Self {
            contact_private_keys: HashMap::new(),
            outgoing_ss: HashMap::new(),
        }
    }

    pub fn generate_for_contact(&mut self, contact_id: &str) -> UnifiedPublicKey {
        let enc = PersistentEncryption::generate();
        let pk = enc.public_key();
        self.contact_private_keys
            .insert(contact_id.to_string(), enc.secret_key_bytes());
        pk
    }

    pub fn get_encryption(&self, contact_id: &str) -> Option<PersistentEncryption> {
        self.contact_private_keys
            .get(contact_id)
            .map(|bytes| PersistentEncryption::from_secret_bytes(*bytes))
    }

    pub fn has_contact(&self, contact_id: &str) -> bool {
        self.contact_private_keys.contains_key(contact_id)
    }

    pub fn store_outgoing_ss(&mut self, contact_id: &str, ss: &[u8; 32]) {
        self.outgoing_ss.insert(contact_id.to_string(), *ss);
    }

    pub fn get_outgoing_ss(&self, contact_id: &str) -> Option<[u8; 32]> {
        self.outgoing_ss.get(contact_id).copied()
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
