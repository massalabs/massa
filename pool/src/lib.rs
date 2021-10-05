// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(map_first_last)]
#![feature(async_closure)]

#[macro_use]
extern crate logging;

mod config;
mod endorsement_pool;
mod error;
mod operation_pool;
mod pool_controller;
mod pool_worker;

pub use config::PoolConfig;
pub use error::PoolError;
pub use pool_controller::{start_pool_controller, PoolCommandSender, PoolManager};
pub use pool_worker::{PoolCommand, PoolStats};

#[cfg(test)]
mod tests;
