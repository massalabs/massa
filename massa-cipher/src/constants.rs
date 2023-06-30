// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! massa-cipher constant values.
//!
//! Read `lib.rs` module documentation for more information.

use pbkdf2::Params;

/// AES-GCM-SIV nonce size.
///
/// A nonce is a single-use value which enables securely encrypting multiple messages under the same key.
/// Nonces need not be random: a counter can be used so long as the values are never repeated under the same key.
pub const NONCE_SIZE: usize = 12;

/// `PBKDF2` salt size.
pub const SALT_SIZE: usize = 16;

/// `PBKDF2` hash parameters.
pub const HASH_PARAMS: Params = Params {
    rounds: 600_000,
    output_length: 32,
};
