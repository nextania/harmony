use argon2::Argon2;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as BASE64};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, aead::Aead};
use zeroize::Zeroizing;

use crate::errors::BoxError;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("decode error: {0}")]
    Decode(#[source] BoxError),

    #[error("invalid key length")]
    InvalidKeyLength,

    #[error("derivation failed")]
    DerivationFailed,

    #[error("decryption failed: {0}")]
    DecryptionFailed(#[source] BoxError),
}

/// Derive key B from the account's `encrypted_key`and the user's password.
pub(crate) fn derive_key_b(
    encrypted_key: &str,
    password: &str,
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    let encrypted_key_bytes = BASE64
        .decode(encrypted_key)
        .map_err(|e| CryptoError::Decode(Box::new(e).into()))?;
    if encrypted_key_bytes.len() != 88 {
        return Err(CryptoError::InvalidKeyLength);
    }
    let argon2 = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        // FIXME: the browser side uses some really weird defaults
        argon2::Params::new(1024, 1, 1, None).unwrap(),
    );
    let salt = &encrypted_key_bytes[..16];
    let mut password_key_a = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(password.as_bytes(), salt, password_key_a.as_mut())
        .map_err(|_| CryptoError::DerivationFailed)?;
    let cipher = XChaCha20Poly1305::new((&*password_key_a).into());
    let nonce: &[u8; 24] = &encrypted_key_bytes[16..40].try_into().unwrap();
    let ciphertext = &encrypted_key_bytes[40..];
    let decrypted = Zeroizing::new(
        cipher
            .decrypt(nonce.into(), ciphertext)
            .map_err(|e| CryptoError::DecryptionFailed(Box::new(e)))?,
    );
    if decrypted.len() != 32 {
        return Err(CryptoError::InvalidKeyLength);
    }
    let mut key_b = Zeroizing::new([0u8; 32]);
    key_b.copy_from_slice(&decrypted);
    Ok(key_b)
}

pub(crate) fn key_b_cipher(key_b: &[u8; 32]) -> XChaCha20Poly1305 {
    XChaCha20Poly1305::new(key_b.into())
}
