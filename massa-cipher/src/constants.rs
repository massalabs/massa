// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! massa-cipher constant values.
//!
//! Read `lib.rs` module documentation for more information.

use pbkdf2::Params;

/// Cipher version
pub(crate)  const VERSION: u32 = 0;

/// AES-GCM-SIV nonce size.
///
/// A nonce is a single-use value which enables securely encrypting multiple messages under the same key.
/// Nonces need not be random: a counter can be used so long as the values are never repeated under the same key.
pub(crate)  const NONCE_SIZE: usize = 12;

/// `PBKDF2` salt size.
pub(crate)  const SALT_SIZE: usize = 12;

/// `PBKDF2` hash parameters.
pub(crate)  const HASH_PARAMS: Params = Params {
    rounds: 10_000,
    output_length: 32,
};
