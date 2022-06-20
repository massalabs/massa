// Copyright (c) 2022 MASSA LABS <info@massa.net>

use aes_gcm_siv::aead::{Aead, NewAead};
use aes_gcm_siv::{Aes256GcmSiv, Key, Nonce};
use massa_hash::Hash;

use crate::error::CipherError;

pub fn decrypt(password: &str, data: &[u8]) -> Result<Vec<u8>, CipherError> {
    let cipher = Aes256GcmSiv::new(Key::from_slice(
        Hash::compute_from(password.as_bytes()).to_bytes(),
    ));
    let mut iter = data.split(|c| c == &b':');
    let nonce = Nonce::from_slice(iter.next().expect("Missing nonce"));
    let text = cipher
        .decrypt(nonce, iter.next().expect("Missing content"))
        .map_err(|_| CipherError::DecryptionError("Wrong password".to_string()))?;
    Ok(text)
}
