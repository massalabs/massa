// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! MASSA Cipher crate
//!
//! `massa-cipher` uses AES-GCM
//!
//! AES-GCM is a state-of-the-art high-performance Authenticated Encryption with Associated Data (AEAD)
//! that provides confidentiality and authenticity.
//!
//! To hash the password before using it as a cipher key, we use the `PBKDF2` key derivation function
//! as specified in [RFC 2898](https://datatracker.ietf.org/doc/html/rfc2898).
//!
//! The AES-GCM crate we use has received one security audit by NCC Group, with no significant findings.

mod constants;
mod decrypt;
mod encrypt;
mod error;

pub use decrypt::decrypt;
pub use encrypt::encrypt;
pub use encrypt::CipherData;
pub use error::CipherError;
