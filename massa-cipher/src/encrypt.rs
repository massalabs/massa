// Copyright (c) 2022 MASSA LABS <info@massa.net>

use aes_gcm_siv::aead::{Aead, NewAead};
use aes_gcm_siv::{Aes256GcmSiv, Key, Nonce};
use massa_hash::Hash;
use rand::{thread_rng, RngCore};

use crate::error::CipherError;

pub fn encrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let cipher = Aes256GcmSiv::new(Key::from_slice(
        Hash::compute_from(password.as_bytes()).to_bytes(),
    ));
    let mut bytes = [0u8; 12];
    thread_rng().fill_bytes(&mut bytes);
    let nonce = Nonce::from_slice(&bytes);
    let text = cipher
        .encrypt(nonce, data.as_ref())
        .map_err(|e| CipherError::EncryptionError(e.to_string()))?;
    let mut content = bytes.to_vec();
    content.extend(b":");
    content.extend(text);
    Ok(content)
}
