use std::sync::{Arc, Mutex};

use crate::HarmonyBindingError;
use crate::error::HarmonyResult;
use crate::models::{HybridPublicKey, UnifiedPublicKey};

#[derive(Debug, Clone, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CryptoError {
    #[error("ciphertext too short or malformed")]
    InvalidCiphertext,
    #[error("public key bytes are malformed")]
    InvalidPublicKey,
    #[error("decryption failed (wrong key or tampered data)")]
    DecryptionFailed,
    #[error("invalid keystore: {reason}")]
    InvalidKeystore { reason: String },
    #[error("key derivation failed: {reason}")]
    KeyDerivation { reason: String },
    #[error("missing key material: {reason}")]
    MissingKey { reason: String },
    #[error("identity key for {user_id} does not match the pinned key")]
    IdentityKeyMismatch { user_id: String },
}

impl From<harmony_api::CryptoError> for CryptoError {
    fn from(error: harmony_api::CryptoError) -> Self {
        match error {
            harmony_api::CryptoError::InvalidCiphertext => CryptoError::InvalidCiphertext,
            harmony_api::CryptoError::InvalidPublicKey => CryptoError::InvalidPublicKey,
            harmony_api::CryptoError::DecryptionFailed => CryptoError::DecryptionFailed,
            harmony_api::CryptoError::InvalidKeystore(reason) => {
                CryptoError::InvalidKeystore { reason }
            }
            harmony_api::CryptoError::KeyDerivation(reason) => {
                CryptoError::KeyDerivation { reason }
            }
            harmony_api::CryptoError::MissingKey(reason) => CryptoError::MissingKey { reason },
            harmony_api::CryptoError::IdentityKeyMismatch(user_id) => {
                CryptoError::IdentityKeyMismatch { user_id }
            }
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct EncapsulateResult {
    pub ciphertext: Vec<u8>,
    pub shared_secret: Vec<u8>,
}

fn to_key32(bytes: Vec<u8>) -> HarmonyResult<[u8; 32]> {
    bytes
        .try_into()
        .map_err(|_| HarmonyBindingError::InvalidInput {
            reason: "expected 32-byte key".into(),
        })
}

#[derive(uniffi::Object)]
pub struct PersistentEncryption {
    inner: harmony_api::PersistentEncryption,
}

#[uniffi::export]
impl PersistentEncryption {
    #[uniffi::constructor]
    pub fn generate() -> Arc<Self> {
        Self {
            inner: harmony_api::PersistentEncryption::generate(),
        }
        .into()
    }

    #[uniffi::constructor]
    pub fn from_secret_bytes(bytes: Vec<u8>) -> HarmonyResult<Arc<Self>> {
        let arr: [u8; harmony_api::crypto::HYBRID_SECRET_KEY_BYTES] =
            bytes
                .try_into()
                .map_err(|_| HarmonyBindingError::InvalidInput {
                    reason: "invalid secret key length".into(),
                })?;
        Ok(Self {
            inner: harmony_api::PersistentEncryption::from_secret_bytes(arr),
        }
        .into())
    }

    pub fn public_key(&self) -> HybridPublicKey {
        self.inner.public_key().into()
    }

    pub fn secret_key_bytes(&self) -> Vec<u8> {
        self.inner.secret_key_bytes().to_vec()
    }

    pub fn decapsulate(&self, ciphertext: Vec<u8>) -> Result<Vec<u8>, CryptoError> {
        Ok(self.inner.decapsulate(&ciphertext)?.to_vec())
    }

    pub fn derive_channel_key(
        &self,
        our_user_id: String,
        our_identity: Vec<u8>,
        their_user_id: String,
        their_pk: UnifiedPublicKey,
        ss_1: Vec<u8>,
        ss_2: Vec<u8>,
    ) -> HarmonyResult<Vec<u8>> {
        let a = to_key32(ss_1)?;
        let b = to_key32(ss_2)?;
        let our_identity = to_key32(our_identity)?;
        let key = self
            .inner
            .derive_channel_key(
                &our_user_id,
                our_identity,
                &their_user_id,
                &their_pk.into(),
                &a,
                &b,
            )
            .map_err(|e| HarmonyBindingError::Crypto {
                reason: e.to_string(),
            })?;
        Ok(key.to_vec())
    }
}

#[uniffi::export]
pub fn encapsulate_to(their_pk: HybridPublicKey) -> Result<EncapsulateResult, CryptoError> {
    let (ciphertext, shared_secret) =
        harmony_api::PersistentEncryption::encapsulate_to(&their_pk.into())?;
    Ok(EncapsulateResult {
        ciphertext: ciphertext.to_vec(),
        shared_secret: shared_secret.to_vec(),
    })
}

#[uniffi::export]
pub fn encrypt_with_key(key: Vec<u8>, plaintext: Vec<u8>, aad: Vec<u8>) -> HarmonyResult<Vec<u8>> {
    let key = to_key32(key)?;
    Ok(harmony_api::PersistentEncryption::encrypt_with_key(
        &key, &plaintext, &aad,
    ))
}

#[uniffi::export]
pub fn decrypt_with_key(key: Vec<u8>, data: Vec<u8>, aad: Vec<u8>) -> Result<Vec<u8>, CryptoError> {
    let key: [u8; 32] = key.try_into().map_err(|_| CryptoError::InvalidCiphertext)?;
    harmony_api::PersistentEncryption::decrypt_with_key(&key, &data, &aad).map_err(Into::into)
}

#[uniffi::export]
pub fn message_aad(channel_id: String, author_id: String) -> Vec<u8> {
    harmony_api::crypto::message_aad(&channel_id, &author_id)
}

#[derive(uniffi::Object)]
pub struct Keystore {
    inner: Mutex<harmony_api::Keystore>,
}

#[uniffi::export]
impl Keystore {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Self {
            inner: Mutex::new(harmony_api::Keystore::new()),
        }
        .into()
    }

    #[uniffi::constructor]
    pub fn from_bytes(bytes: Vec<u8>) -> HarmonyResult<Arc<Self>> {
        let ks = harmony_api::Keystore::from_bytes(&bytes).map_err(|e| {
            HarmonyBindingError::InvalidInput {
                reason: format!("failed to deserialize keystore: {e}"),
            }
        })?;
        Ok(Self {
            inner: Mutex::new(ks),
        }
        .into())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.inner.lock().unwrap().to_bytes()
    }

    pub fn generate_contact(&self, contact_id: String) -> UnifiedPublicKey {
        let mut ks = self.inner.lock().unwrap();
        let (public_key, private_key) = ks.generate();
        ks.store_contact_key(&contact_id, private_key);
        public_key.into()
    }

    pub fn has_contact(&self, contact_id: String) -> bool {
        self.inner.lock().unwrap().has_contact(&contact_id)
    }

    pub fn get_encryption(&self, contact_id: String) -> Option<Arc<PersistentEncryption>> {
        self.inner
            .lock()
            .unwrap()
            .get_encryption(&contact_id)
            .map(|inner| PersistentEncryption { inner }.into())
    }

    pub fn store_outgoing_ss(&self, contact_id: String, ss: Vec<u8>) -> HarmonyResult<()> {
        let ss = to_key32(ss)?;
        self.inner
            .lock()
            .unwrap()
            .store_outgoing_ss(&contact_id, &ss);
        Ok(())
    }

    pub fn get_outgoing_ss(&self, contact_id: String) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .get_outgoing_ss(&contact_id)
            .map(|k| k.to_vec())
    }

    pub fn store_direct_key(&self, key_id: String, key: Vec<u8>) -> HarmonyResult<()> {
        let key = to_key32(key)?;
        self.inner.lock().unwrap().store_direct_key(&key_id, key);
        Ok(())
    }

    pub fn get_direct_key(&self, key_id: String) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .get_direct_key(&key_id)
            .map(|k| k.to_vec())
    }

    pub fn store_group_key(&self, channel_id: String, key: Vec<u8>) -> HarmonyResult<()> {
        let key = to_key32(key)?;
        self.inner
            .lock()
            .unwrap()
            .store_group_key(&channel_id, &key);
        Ok(())
    }

    pub fn get_group_key(&self, channel_id: String) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .get_group_key(&channel_id)
            .map(|k| k.to_vec())
    }

    pub fn identity_seed(&self) -> Vec<u8> {
        self.inner.lock().unwrap().identity_seed().to_vec()
    }

    pub fn identity_verifying_key(&self) -> Vec<u8> {
        self.inner.lock().unwrap().identity_verifying_key().to_vec()
    }

    pub fn pin_identity_key(&self, user_id: String, key: Vec<u8>) -> HarmonyResult<()> {
        let key = to_key32(key)?;
        self.inner
            .lock()
            .unwrap()
            .pin_identity_key(&user_id, key)
            .map_err(Into::into)
    }

    pub fn get_pinned_identity_key(&self, user_id: String) -> Option<Vec<u8>> {
        self.inner
            .lock()
            .unwrap()
            .get_pinned_identity_key(&user_id)
            .map(|k| k.to_vec())
    }
}

#[uniffi::export]
pub fn safety_number(
    user_a: String,
    identity_a: Vec<u8>,
    user_b: String,
    identity_b: Vec<u8>,
) -> HarmonyResult<String> {
    let a = to_key32(identity_a)?;
    let b = to_key32(identity_b)?;
    Ok(harmony_api::crypto::safety_number(&user_a, &a, &user_b, &b))
}
