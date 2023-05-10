// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Definition and exports of the PoS types and errors.
//!
//! Define also the Selector worker that compute in background the draws for
//! the future cycles

#![warn(missing_docs)]

mod config;
mod controller_traits;
mod error;
mod types;

pub use config::FactoryConfig;
pub use controller_traits::FactoryManager;
pub use types::FactoryChannels;
// pub(crate)  use error::*;
// pub(crate)  use types::*;

/// Tests utils
#[cfg(feature = "testing")]
pub mod test_exports;
