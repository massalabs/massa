// Copyright (c) 2021 MASSA LABS <info@massa.net>

use displaydoc::Display;
use models::Address;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum WalletError {
    /// IO error: {0}
    IOError(#[from] std::io::Error),
    /// JSON error: {0}
    JSONError(#[from] serde_json::Error),
    /// Serde Sq error: {0}
    SerdeqsError(#[from] serde_qs::Error),
    /// Models error: {0}
    ModelsError(#[from] models::ModelsError),
    /// Crypto error: {0}
    CryptoError(#[from] crypto::CryptoError),
    /// Missing key error: {0}
    MissingKeyError(Address),
}
