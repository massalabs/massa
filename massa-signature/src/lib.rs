// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Signature management

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
mod error;
mod signature_impl;

pub use error::MassaSignatureError;
pub use signature_impl::{
    KeyPair, PublicKey, PublicKeyDeserializer, Signature, SignatureDeserializer,
    KEYPAIR_SIZE_BYTES, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES,
};
