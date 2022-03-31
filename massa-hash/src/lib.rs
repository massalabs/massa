// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Hash management crate
#![warn(missing_docs)]
pub use error::MassaHashError;
pub use settings::HASH_SIZE_BYTES;

mod error;
mod hash;
pub use hash::*;
mod settings;
