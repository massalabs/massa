// Copyright (c) 2021 MASSA LABS <info@massa.net>

mod error;
pub mod hash;
pub mod signature;
pub use error::CryptoError;
pub use signature::{derive_public_key, generate_random_private_key, sign, verify_signature};
