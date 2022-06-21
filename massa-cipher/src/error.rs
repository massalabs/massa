// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

/// Cipher error
#[derive(Display, Error, Debug)]
pub enum CipherError {
    /// Encryption error: {0}
    EncryptionError(String),
    /// Decryption error: {0}
    DecryptionError(String),
}
