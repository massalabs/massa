// Copyright (c) 2022 MASSA LABS <info@massa.net>

use displaydoc::Display;
use thiserror::Error;

#[non_exhaustive]
#[derive(Display, Error, Debug)]
pub enum MassaSignatureError {
    /// parsing error : {0}
    ParsingError(String),

    /// secp256k1 engine error: {0}
    EngineError(#[from] secp256k1::Error),

    /// Wrong prefix for hash: expected {0}, got {1}
    WrongPrefix(String, String),
}
