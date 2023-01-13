// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Hash management crate

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
pub use error::MassaHashError;
pub use settings::HASHV1_SIZE_BYTES;
pub use settings::HASHV2_SIZE_BYTES;

mod error;
mod hash;
mod hash_v2;
pub use hash::*;
pub use hash_v2::*;
mod settings;
