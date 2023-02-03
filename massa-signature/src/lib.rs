// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Signature management

//#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![allow(missing_docs)]
#![allow(dead_code)]
mod error;
//mod signature_impl;

#[macro_use]
mod multi;

pub use error::MassaSignatureError;
/*pub use signature_impl::{
    verify_signature_batch, KeyPair, PublicKey, PublicKeyDeserializer, Signature,
    SignatureDeserializer, PUBLIC_KEY_SIZE_BYTES, SECRET_KEY_BYTES_SIZE, SIGNATURE_SIZE_BYTES,
};*/
pub use multi::{
    verify_signature_batch, KeyPair, PublicKey, PublicKeyDeserializer, PublicKeyV0, PublicKeyV1,
    Signature, SignatureDeserializer,
};

/// TODO: Backcompatibility for the code
pub const PUBLIC_KEY_SIZE_BYTES_V1: usize = 32;
/// TODO: Backcompatibility for the code
pub const SIGNATURE_SIZE_BYTES_V1: usize = 32;
