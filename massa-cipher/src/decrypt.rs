// Copyright (c) 2022 MASSA LABS <info@massa.net>

use aes_gcm_siv::aead::{Aead, NewAead};
use aes_gcm_siv::{Aes256GcmSiv, Key, Nonce};
use massa_hash::Hash;

use crate::constants::NONCE_SIZE;
use crate::error::CipherError;

/// Decryption function using AES-GCM-SIV cipher.
///
/// Read `lib.rs` module documentation for more information.
pub fn decrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let cipher = Aes256GcmSiv::new(Key::from_slice(
        Hash::compute_from(password.as_bytes()).to_bytes(),
    ));
    let nonce = Nonce::from_slice(data.get(..NONCE_SIZE).ok_or_else(|| {
        CipherError::DecryptionError(
            "wallet file truncated: nonce missing or incomplete".to_string(),
        )
    })?);
    let decrypted_bytes = cipher
        .decrypt(
            nonce,
            data.get(NONCE_SIZE..).ok_or_else(|| {
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
