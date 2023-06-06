// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use massa_models::address::Address;
use thiserror::Error;

/// wallet error
#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum WalletError {
    /// IO error: {0}
    IOError(#[from] std::io::Error),
    /// YAML error: {0}
    YAMLError(#[from] serde_yaml::Error),
    /// Serde Sq error: {0}
    SerdeqsError(#[from] serde_qs::Error),
    /// Models error: {0}
    ModelsError(#[from] massa_models::error::ModelsError),
    /// `MassaHash` error: {0}
    MassaHashError(#[from] massa_hash::MassaHashError),
    /// `MassaSignature` error: {0}
    MassaSignatureError(#[from] massa_signature::MassaSignatureError),
    /// Missing key error: {0}
    MissingKeyError(Address),
    /// `MassaCipher` error: {0}
    MassaCipherError(#[from] massa_cipher::CipherError),
}
