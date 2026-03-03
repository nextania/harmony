use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead, AeadCore, OsRng},
};
use hkdf::Hkdf;
use ml_kem::{
    Decapsulate, DecapsulationKey768, Encapsulate, EncapsulationKey768, Kem, KeyExport, MlKem768,
    Seed, TryKeyInit,
};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

pub use harmony_api::{Encapsulated, MLKEM768_CT_BYTES, MLKEM768_EK_BYTES, UnifiedPublicKey};

const HKDF_INFO: &[u8] = b"harmony-persistent-encryption-v1";

pub const HYBRID_PUBLIC_KEY_BYTES: usize = 32 + MLKEM768_EK_BYTES;
pub const HYBRID_SECRET_KEY_BYTES: usize = 32 + 64;

pub fn unified_pk_from_bytes(bytes: &[u8; HYBRID_PUBLIC_KEY_BYTES]) -> UnifiedPublicKey {
    let mut x25519 = [0u8; 32];
    x25519.copy_from_slice(&bytes[..32]);
    let mlkem = bytes[32..].to_vec();
    UnifiedPublicKey { x25519, mlkem }
}

pub fn unified_pk_to_bytes(pk: &UnifiedPublicKey) -> [u8; HYBRID_PUBLIC_KEY_BYTES] {
    let mut out = [0u8; HYBRID_PUBLIC_KEY_BYTES];
    out[..32].copy_from_slice(&pk.x25519);
    out[32..].copy_from_slice(&pk.mlkem);
    out
}

pub struct PersistentEncryption {
    x25519_secret: StaticSecret,
    x25519_public: PublicKey,
    mlkem_dk: DecapsulationKey768,
    mlkem_ek: EncapsulationKey768,
}

impl PersistentEncryption {
    pub fn generate() -> Self {
        let x25519_secret = StaticSecret::random_from_rng(OsRng);
        let x25519_public = PublicKey::from(&x25519_secret);
        let (mlkem_dk, mlkem_ek) = MlKem768::generate_keypair();
        Self {
            x25519_secret,
            x25519_public,
            mlkem_dk,
            mlkem_ek,
        }
    }

    pub fn from_secret_bytes(bytes: [u8; HYBRID_SECRET_KEY_BYTES]) -> Self {
        let mut x25519_bytes = [0u8; 32];
        x25519_bytes.copy_from_slice(&bytes[..32]);
        let x25519_secret = StaticSecret::from(x25519_bytes);
        let x25519_public = PublicKey::from(&x25519_secret);

        let seed: Seed = bytes[32..]
            .try_into()
            .expect("seed slice is exactly 64 bytes");
        let mlkem_dk = DecapsulationKey768::from_seed(seed);
        let mlkem_ek = mlkem_dk.encapsulation_key().clone();

        Self {
            x25519_secret,
            x25519_public,
            mlkem_dk,
            mlkem_ek,
        }
    }

    pub fn public_key(&self) -> UnifiedPublicKey {
        let x25519 = *self.x25519_public.as_bytes();
        let ek_bytes = self.mlkem_ek.to_bytes();
        let mlkem: Vec<u8> = ek_bytes.as_slice().to_vec();
        UnifiedPublicKey { x25519, mlkem }
    }

    pub fn secret_key_bytes(&self) -> [u8; HYBRID_SECRET_KEY_BYTES] {
        // [ x25519_sk (32) | mlkem_seed (64) ]
        let mut out = [0u8; HYBRID_SECRET_KEY_BYTES];
        out[..32].copy_from_slice(&self.x25519_secret.to_bytes());
        out[32..].copy_from_slice(self.mlkem_dk.to_bytes().as_ref());
        out
    }

    pub fn encapsulate_to(their_pk: &UnifiedPublicKey) -> (Encapsulated, [u8; 32]) {
        let their_mlkem_ek = EncapsulationKey768::new_from_slice(&their_pk.mlkem)
            .expect("encapsulation key bytes are valid");
        let (ct, ss) = their_mlkem_ek.encapsulate();
        let ct_bytes: Vec<u8> = ct.as_slice().to_vec();
        let mut ss_bytes = [0u8; 32];
        ss_bytes.copy_from_slice(ss.as_ref());
        (ct_bytes, ss_bytes)
    }

    pub fn decapsulate(&self, ct: &[u8]) -> [u8; 32] {
        let mlkem_ct: ml_kem::Ciphertext<MlKem768> = ct
            .try_into()
            .expect("ciphertext is exactly MLKEM768_CT_BYTES");
        let ss = self.mlkem_dk.decapsulate(&mlkem_ct);
        let mut ss_bytes = [0u8; 32];
        ss_bytes.copy_from_slice(ss.as_ref());
        ss_bytes
    }

    pub fn derive_channel_key(
        &self,
        their_pk: &UnifiedPublicKey,
        ss_1: &[u8; 32],
        ss_2: &[u8; 32],
    ) -> [u8; 32] {
        let their_x25519_pk = PublicKey::from(their_pk.x25519);
        let ss_x25519 = self.x25519_secret.diffie_hellman(&their_x25519_pk);

        let mut ikm = [0u8; 96];
        ikm[..32].copy_from_slice(ss_x25519.as_bytes());
        ikm[32..64].copy_from_slice(ss_1);
        ikm[64..].copy_from_slice(ss_2);
        let hkdf = Hkdf::<Sha256>::new(None, &ikm);
        let mut key = [0u8; 32];
        hkdf.expand(HKDF_INFO, &mut key)
            .expect("HKDF expand should not fail for 32-byte output");
        key
    }

    pub fn encrypt_with_key(key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .expect("ChaCha20-Poly1305 encryption should not fail");

        // [ nonce (12) | chacha_ct ]
        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        out
    }

    pub fn decrypt_with_key(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if data.len() < 12 {
            return Err(CryptoError::InvalidCiphertext);
        }
        let (nonce_bytes, chacha_ct) = data.split_at(12);
        let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);
        let cipher = ChaCha20Poly1305::new(key.into());
        cipher
            .decrypt(nonce, chacha_ct)
            .map_err(|_| CryptoError::DecryptionFailed)
    }
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
