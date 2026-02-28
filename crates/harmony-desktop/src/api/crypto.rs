use std::collections::HashMap;

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, OsRng},
};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

const HKDF_INFO: &[u8] = b"harmony-persistent-encryption-v1";

#[derive(Clone)]
pub struct PersistentEncryption {
    secret: StaticSecret,
    public: PublicKey,
    derived_keys: HashMap<[u8; 32], [u8; 32]>,
}

impl PersistentEncryption {
    pub fn generate() -> Self {
        let secret = StaticSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        Self {
            secret,
            public,
            derived_keys: HashMap::new(),
        }
    }

    pub fn from_secret_bytes(secret_bytes: [u8; 32]) -> Self {
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);
        Self {
            secret,
            public,
            derived_keys: HashMap::new(),
        }
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    pub fn public_key_bytes(&self) -> [u8; 32] {
        *self.public.as_bytes()
    }

    pub fn secret_key_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }

    pub fn derive_key(&mut self, their_public_key: &[u8; 32]) -> [u8; 32] {
        if let Some(key) = self.derived_keys.get(their_public_key) {
            return *key;
        }

        let their_pk = PublicKey::from(*their_public_key);
        let shared_secret = self.secret.diffie_hellman(&their_pk);

        let hkdf = Hkdf::<Sha256>::new(None, shared_secret.as_bytes());
        let mut okm = [0u8; 32];
        hkdf.expand(HKDF_INFO, &mut okm)
            .expect("HKDF expand should not fail for 32-byte output");

        self.derived_keys.insert(*their_public_key, okm);
        okm
    }

    pub fn encrypt(&mut self, plaintext: &[u8], their_public_key: &[u8; 32]) -> Vec<u8> {
        let key = self.derive_key(their_public_key);
        encrypt_raw(plaintext, &key)
    }

    pub fn decrypt(
        &mut self,
        ciphertext: &[u8],
        their_public_key: &[u8; 32],
    ) -> Result<Vec<u8>, CryptoError> {
        let key = self.derive_key(their_public_key);
        decrypt_raw(ciphertext, &key)
    }
}

pub fn encrypt_raw(plaintext: &[u8], key: &[u8; 32]) -> Vec<u8> {
    use chacha20poly1305::aead::AeadCore;

    let cipher = ChaCha20Poly1305::new(key.into());
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .expect("ChaCha20-Poly1305 encryption should not fail");

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out
}

pub fn decrypt_raw(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, CryptoError> {
    if data.len() < 12 {
        return Err(CryptoError::InvalidCiphertext);
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
    let cipher = ChaCha20Poly1305::new(key.into());
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)
}

#[derive(Debug, Clone)]
pub enum CryptoError {
    InvalidCiphertext,
    DecryptionFailed,
}

impl std::fmt::Display for CryptoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CryptoError::InvalidCiphertext => write!(f, "ciphertext too short or malformed"),
            CryptoError::DecryptionFailed => {
                write!(f, "decryption failed (wrong key or tampered data)")
            }
        }
    }
}

impl std::error::Error for CryptoError {}
