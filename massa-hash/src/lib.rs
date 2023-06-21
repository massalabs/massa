// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Hash management crate

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
pub use error::MassaHashError;
pub use settings::HASH_SIZE_BYTES;
pub use settings::XOF_HASH_SIZE_BYTES;

mod error;
mod hash;
pub use hash::*;
mod settings;
