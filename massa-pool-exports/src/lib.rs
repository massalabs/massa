//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Pool of operation and endorsements waiting to be included in a block
//!
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_first_last)]

mod config;
mod controller_traits;
mod error;

pub use config::PoolConfig;
pub use controller_traits::{PoolController, PoolManager};
pub use error::PoolError;

/// Test utils
#[cfg(feature = "testing")]
/// Exports related to tests as Mocks and configurations
pub mod test_exports;
