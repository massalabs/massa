// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(map_first_last)]
#![feature(async_closure)]

#[macro_use]
extern crate logging;

pub use config::PoolConfig;
pub use error::PoolError;
pub use models::stats::PoolStats;
pub use pool_controller::{start_pool_controller, PoolCommandSender, PoolManager};
pub use pool_worker::PoolCommand;

mod config;
mod endorsement_pool;
mod error;
mod operation_pool;
mod pool_controller;
mod pool_worker;

#[cfg(test)]
mod tests;
