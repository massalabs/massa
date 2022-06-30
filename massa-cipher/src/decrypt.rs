// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! massa-cipher decryption module.
//!
//! Read `lib.rs` module documentation for more information.

use aes_gcm::aead::{Aead, NewAead};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use pbkdf2::{
    password_hash::{PasswordHasher, SaltString},
    Pbkdf2,
};

use crate::constants::{B64_SALT_SIZE, HASH_PARAMS, NONCE_SIZE};
use crate::error::CipherError;

/// Decryption function using AES-GCM-SIV cipher.
///
/// Read `lib.rs` module documentation for more information.
pub fn decrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let salt_data = data.get(..B64_SALT_SIZE).ok_or_else(|| {
        CipherError::DecryptionError(
            "wallet file truncated: salt missing or incomplete".to_string(),
        )
    })?;
    let salt = SaltString::new(std::str::from_utf8(salt_data)?)
        .map_err(|e| CipherError::DecryptionError(e.to_string()))?;
    let password_hash = Pbkdf2
        .hash_password_customized(password.as_bytes(), None, None, HASH_PARAMS, &salt)
        .map_err(|e| CipherError::DecryptionError(e.to_string()))?
        .hash
        .expect("content is missing after a successful hash");
    let cipher = Aes256Gcm::new(Key::from_slice(password_hash.as_bytes()));
    let nonce_end_index = B64_SALT_SIZE + NONCE_SIZE;
    let nonce = Nonce::from_slice(data.get(B64_SALT_SIZE..nonce_end_index).ok_or_else(|| {
        CipherError::DecryptionError(
            "wallet file truncated: nonce missing or incomplete".to_string(),
        )
    })?);
    let decrypted_bytes = cipher
        .decrypt(
            nonce,
            data.get(nonce_end_index..).ok_or_else(|| {
                CipherError::DecryptionError(
                    "wallet file truncated: encrypted data missing or incomplete".to_string(),
                )
            })?,
        )
        .map_err(|_| {
            CipherError::DecryptionError("wrong password or corrupted data".to_string())
        })?;
    Ok(decrypted_bytes)
}
