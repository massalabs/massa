// Copyright (c) 2022 MASSA LABS <info@massa.net>

pub mod signature_impl;
pub use signature_impl::{
    derive_public_key, generate_random_private_key, sign, verify_signature, PrivateKey, PublicKey,
    Signature, PRIVATE_KEY_SIZE_BYTES, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES,
};
