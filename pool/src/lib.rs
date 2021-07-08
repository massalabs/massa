#[macro_use]
extern crate logging;

mod config;
mod error;
mod pool_controller;
mod pool_worker;

pub use config::PoolConfig;
pub use error::PoolError;
pub use pool_controller::{start_pool_controller, PoolCommandSender, PoolManager};

#[cfg(test)]
mod tests;
