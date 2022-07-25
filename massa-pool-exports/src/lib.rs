// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Pool of operation and endorsements waiting to be included in a block
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_first_last)]

mod error;
mod controller_traits;
mod config;

pub use error::PoolError;
pub use controller_traits::{start_pool_controller, PoolController, PoolManager};
pub use config::PoolConfig;


#[cfg(test)]
mod tests;
