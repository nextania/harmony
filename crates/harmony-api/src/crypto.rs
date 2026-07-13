use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ed25519_dalek::SigningKey;
use getrandom::{SysRng, rand_core::UnwrapErr};
use hkdf::Hkdf;
use ml_kem::{
    Decapsulate, DecapsulationKey768, Encapsulate, EncapsulationKey768, Generate, Kem, KeyExport,
    MlKem768, Seed, TryKeyInit,
};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

pub use crate::models::{
    Encapsulated, HybridPublicKey, MLKEM768_CT_BYTES, MLKEM768_EK_BYTES, UnifiedPublicKey,
};

const CHANNEL_KEY_SALT: &[u8] = b"harmony-persistent-channel-key-v1";

pub const GROUP_METADATA_AAD: &[u8] = b"harmony-group-metadata-v1";

/// Build the AAD that binds a message ciphertext to its channel and sender.
pub fn message_aad(channel_id: &str, author_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(14 + 16 + channel_id.len() + author_id.len());
    aad.extend_from_slice(b"harmony-msg-v1");
    aad.extend_from_slice(&(channel_id.len() as u64).to_le_bytes());
    aad.extend_from_slice(channel_id.as_bytes());
    aad.extend_from_slice(&(author_id.len() as u64).to_le_bytes());
    aad.extend_from_slice(author_id.as_bytes());
    aad
}

pub const HYBRID_PUBLIC_KEY_BYTES: usize = 32 + MLKEM768_EK_BYTES;
pub const HYBRID_SECRET_KEY_BYTES: usize = 32 + 64;

/// Derive the Ed25519 verifying (public) key from a 32-byte identity seed.
pub fn identity_verifying_key(seed: &[u8; 32]) -> [u8; 32] {
    SigningKey::from_bytes(seed).verifying_key().to_bytes()
}

/// Compute the 40-digit code for a contact pair to verify authenticity.
pub fn safety_number(
    user_a: &str,
    identity_a: &[u8; 32],
    user_b: &str,
    identity_b: &[u8; 32],
) -> String {
    use sha2::Digest;
    let ours = (user_a.as_bytes(), identity_a);
    let theirs = (user_b.as_bytes(), identity_b);
    let (first, second) = if ours <= theirs {
        (ours, theirs)
    } else {
        (theirs, ours)
    };
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"harmony-safety-number-v1");
    for (id, key) in [first, second] {
        hasher.update((id.len() as u64).to_le_bytes());
        hasher.update(id);
        hasher.update(key);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(8 * 5 + 7);
    for (i, chunk) in digest.chunks_exact(4).enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let n = u32::from_be_bytes(chunk.try_into().expect("chunk is 4 bytes")) % 100_000;
        out.push_str(&format!("{n:05}"));
    }
    out
}

pub fn hybrid_pk_from_bytes(bytes: &[u8; HYBRID_PUBLIC_KEY_BYTES]) -> HybridPublicKey {
    let mut x25519 = [0u8; 32];
    x25519.copy_from_slice(&bytes[..32]);
    let mlkem: [u8; 1184] = bytes[32..].try_into().unwrap();
    HybridPublicKey { x25519, mlkem }
}

pub fn hybrid_pk_to_bytes(
    pk: &HybridPublicKey,
) -> Result<[u8; HYBRID_PUBLIC_KEY_BYTES], CryptoError> {
    if pk.mlkem.len() != MLKEM768_EK_BYTES {
        return Err(CryptoError::InvalidPublicKey);
    }
    let mut out = [0u8; HYBRID_PUBLIC_KEY_BYTES];
    out[..32].copy_from_slice(&pk.x25519);
    out[32..].copy_from_slice(&pk.mlkem);
    Ok(out)
}

pub struct PersistentEncryption {
    x25519_secret: StaticSecret,
    x25519_public: PublicKey,
    mlkem_dk: DecapsulationKey768,
    mlkem_ek: EncapsulationKey768,
}

impl PersistentEncryption {
    pub fn generate() -> Self {
        let x25519_secret = StaticSecret::random_from_rng(&mut UnwrapErr(SysRng));
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

    /// The hybrid KEM public key.
    pub fn public_key(&self) -> HybridPublicKey {
        let x25519 = *self.x25519_public.as_bytes();
        let mlkem = self.mlkem_ek.to_bytes().0;
        HybridPublicKey { x25519, mlkem }
    }

    pub fn secret_key_bytes(&self) -> [u8; HYBRID_SECRET_KEY_BYTES] {
        // [ x25519_sk (32) | mlkem_seed (64) ]
        let mut out = [0u8; HYBRID_SECRET_KEY_BYTES];
        out[..32].copy_from_slice(&self.x25519_secret.to_bytes());
        out[32..].copy_from_slice(self.mlkem_dk.to_bytes().as_ref());
        out
    }

    /// Encapsulate to a peer's ML-KEM public key.
    pub fn encapsulate_to(
        their_pk: &HybridPublicKey,
    ) -> Result<(Encapsulated, [u8; 32]), CryptoError> {
        let their_mlkem_ek = EncapsulationKey768::new_from_slice(&their_pk.mlkem)
            .map_err(|_| CryptoError::InvalidPublicKey)?;
        let (ct, ss) = their_mlkem_ek.encapsulate();
        let mut ss_bytes = [0u8; 32];
        ss_bytes.copy_from_slice(ss.as_ref());
        Ok((ct.0, ss_bytes))
    }

    /// Decapsulate a peer-supplied ciphertext.
    pub fn decapsulate(&self, ct: &[u8]) -> Result<[u8; 32], CryptoError> {
        let mlkem_ct: ml_kem::Ciphertext<MlKem768> =
            ct.try_into().map_err(|_| CryptoError::InvalidCiphertext)?;
        let ss = self.mlkem_dk.decapsulate(&mlkem_ct);
        let mut ss_bytes = [0u8; 32];
        ss_bytes.copy_from_slice(ss.as_ref());
        Ok(ss_bytes)
    }

    /// Derive the shared direct-channel key from the X25519 DH secret and the
    /// two ML-KEM shared secrets.
    pub fn derive_channel_key(
        &self,
        our_user_id: &str,
        our_identity: [u8; 32],
        their_user_id: &str,
        their_pk: &UnifiedPublicKey,
        ss_1: &[u8; 32],
        ss_2: &[u8; 32],
    ) -> Result<[u8; 32], CryptoError> {
        let (ss_a, ss_b) = if ss_1 <= ss_2 {
            (ss_1, ss_2)
        } else {
            (ss_2, ss_1)
        };
        let their_x25519_pk = PublicKey::from(their_pk.hybrid.x25519);
        let ss_x25519 = self.x25519_secret.diffie_hellman(&their_x25519_pk);

        let our_pk_bytes = hybrid_pk_to_bytes(&self.public_key())?;
        let their_pk_bytes = hybrid_pk_to_bytes(&their_pk.hybrid)?;
        let ours = (our_user_id.as_bytes(), our_pk_bytes, our_identity);
        let theirs = (their_user_id.as_bytes(), their_pk_bytes, their_pk.ed25519);
        let (first, second) = if (ours.0, &ours.1) <= (theirs.0, &theirs.1) {
            (&ours, &theirs)
        } else {
            (&theirs, &ours)
        };
        let mut info = Vec::with_capacity(
            2 * (8 + HYBRID_PUBLIC_KEY_BYTES + 32) + first.0.len() + second.0.len(),
        );
        for (user_id, pk, identity) in [first, second] {
            info.extend_from_slice(&(user_id.len() as u64).to_le_bytes());
            info.extend_from_slice(user_id);
            info.extend_from_slice(pk.as_slice());
            // TODO: is this an issue
            info.extend_from_slice(identity);
        }

        let mut ikm = [0u8; 96];
        ikm[..32].copy_from_slice(ss_x25519.as_bytes());
        ikm[32..64].copy_from_slice(ss_a);
        ikm[64..].copy_from_slice(ss_b);
        let hkdf = Hkdf::<Sha256>::new(Some(CHANNEL_KEY_SALT), &ikm);
        let mut key = [0u8; 32];
        hkdf.expand(&info, &mut key)
            .expect("HKDF expand should not fail for 32-byte output");
        Ok(key)
    }

    /// Encrypt `plaintext`, authenticating `aad`.
    pub fn encrypt_with_key(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
        let cipher = XChaCha20Poly1305::new(key.into());
        let nonce = XNonce::generate();
        let ciphertext = cipher
            .encrypt(
                &nonce,
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .expect("XChaCha20-Poly1305 encryption should not fail");

        // [ nonce (24) | chacha_ct ]
        let mut out = Vec::with_capacity(24 + ciphertext.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ciphertext);
        out
    }

    pub fn decrypt_with_key(
        key: &[u8; 32],
        data: &[u8],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        if data.len() < 24 {
            return Err(CryptoError::InvalidCiphertext);
        }
        let (nonce_bytes, chacha_ct) = data.split_at(24);
        let nonce: &[u8; 24] = nonce_bytes.try_into().unwrap();
        let cipher = XChaCha20Poly1305::new(key.into());
        cipher
            .decrypt(
                nonce.into(),
                Payload {
                    msg: chacha_ct,
                    aad,
                },
            )
            .map_err(|_| CryptoError::DecryptionFailed)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum CryptoError {
    #[error("ciphertext too short or malformed")]
    InvalidCiphertext,
    #[error("public key bytes are malformed")]
    InvalidPublicKey,
    #[error("decryption failed (wrong key or tampered data)")]
    DecryptionFailed,
    #[error("invalid keystore: {0}")]
    InvalidKeystore(String),
    #[error("key derivation failed: {0}")]
    KeyDerivation(String),
    #[error("missing key material: {0}")]
    MissingKey(String),
    #[error("identity key for {0} does not match the pinned key")]
    IdentityKeyMismatch(String),
}
