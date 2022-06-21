// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Cipher crate
//!
//! `massa-cipher` uses AES-GCM-SIV ([RFC 8452](https://datatracker.ietf.org/doc/html/rfc8452)).
//!
//! AES-GCM-SIV is a state-of-the-art high-performance Authenticated Encryption with Associated Data (AEAD)
//! cipher which also provides nonce reuse misuse resistance.
//! Suitable as a general purpose symmetric encryption cipher, AES-GCM-SIV also removes many of the sharp edges of AES-GCM.
//!
//! A nonce is a single-use value which enables securely encrypting multiple messages under the same key.
//! Nonces need not be random: a counter can be used, so long as the values are never repeated under the same key.
//!
//! No complete security audits of the crate we use has been performed.
//! But some of this crate's dependencies were audited by by NCC Group as part of an audit of the AES-GCM crate

mod constants;
mod decrypt;
mod encrypt;
mod error;

pub use decrypt::decrypt;
pub use encrypt::encrypt;
pub use error::CipherError;
