// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Pool of operation and endorsements waiting to be included in a block
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_first_last)]
#![feature(async_closure)]

#[macro_use]
extern crate massa_logging;

pub use error::PoolError;
pub use pool_controller::{start_pool_controller, PoolCommandSender, PoolManager};
pub use pool_worker::PoolCommand;
pub use settings::{PoolConfig, PoolSettings};

mod endorsement_pool;
mod error;
mod operation_pool;
mod pool_controller;
mod pool_worker;
mod settings;

#[cfg(test)]
mod tests;
