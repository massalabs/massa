// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![warn(missing_docs)]
pub use error::MassaHashError;
pub use settings::HASH_SIZE_BYTES;

mod error;
pub mod hash;
mod settings;
