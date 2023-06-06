// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! massa-cipher decryption module.
//!
//! Read `lib.rs` module documentation for more information.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use pbkdf2::{
    password_hash::{PasswordHasher, SaltString},
    Pbkdf2,
};

use crate::constants::HASH_PARAMS;
use crate::encrypt::CipherData;
use crate::error::CipherError;

/// Decryption function using AES-GCM cipher.
///
/// Read `lib.rs` module documentation for more information.
pub fn decrypt(password: &str, data: CipherData) -> Result<Vec<u8>, CipherError> {
    // parse PBKDF2 salt
    let salt_data = data.salt;
    let salt = SaltString::new(std::str::from_utf8(&salt_data)?)
        .map_err(|e| CipherError::DecryptionError(e.to_string()))?;

    // compute PBKDF2 password hash
    let password_hash = Pbkdf2
        .hash_password_customized(password.as_bytes(), None, None, HASH_PARAMS, &salt)
        .map_err(|e| CipherError::DecryptionError(e.to_string()))?
        .hash
        .expect("content is missing after a successful hash");

    // parse AES-GCM nonce
    let nonce = Nonce::from_slice(&data.nonce);

    // decrypt the data
    let cipher = Aes256Gcm::new_from_slice(password_hash.as_bytes()).expect("invalid size key");
    let decrypted_bytes = cipher
        .decrypt(nonce, data.encrypted_bytes.as_ref())
        .map_err(|_| {
            CipherError::DecryptionError("wrong password or corrupted data".to_string())
        })?;
    Ok(decrypted_bytes)
}
