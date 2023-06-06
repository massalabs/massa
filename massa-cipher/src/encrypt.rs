// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! massa-cipher encryption module.
//!
//! Read `lib.rs` module documentation for more information.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use pbkdf2::password_hash::Salt;
use pbkdf2::{password_hash::PasswordHasher, Pbkdf2};
use rand::{distributions::Alphanumeric, thread_rng, Rng, RngCore};

use crate::constants::{HASH_PARAMS, NONCE_SIZE, SALT_SIZE};
use crate::error::CipherError;

pub struct CipherData {
    pub salt: [u8; SALT_SIZE],
    pub nonce: [u8; NONCE_SIZE],
    pub encrypted_bytes: Vec<u8>,
}

/// Encryption function using AES-GCM cipher.
///
/// Read `lib.rs` module documentation for more information.
pub fn encrypt(password: &str, data: &[u8]) -> Result<CipherData, CipherError> {
    // generate the PBKDF2 salt
    let raw_salt: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .take(SALT_SIZE)
        .map(char::from)
        .collect();
    let salt = Salt::new(&raw_salt).expect("salt creation failed");

    // compute PBKDF2 password hash
    let password_hash = Pbkdf2
        .hash_password_customized(password.as_bytes(), None, None, HASH_PARAMS, salt)
        .map_err(|e| CipherError::EncryptionError(e.to_string()))?
        .hash
        .expect("content is missing after a successful hash");

    // generate the AES-GCM nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    // encrypt the data
    let cipher = Aes256Gcm::new_from_slice(password_hash.as_bytes()).expect("invalid key length");
    let encrypted_bytes = cipher
        .encrypt(nonce, data.as_ref())
        .map_err(|e| CipherError::EncryptionError(e.to_string()))?;

    // build the encryption result
    let result = CipherData {
        salt: salt.as_bytes().try_into().expect("invalid salt length"),
        nonce: nonce_bytes,
        encrypted_bytes,
    };
    Ok(result)
}
