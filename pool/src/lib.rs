#[macro_use]
extern crate logging;

mod config;
mod pool_controller;
mod pool_worker;
mod error;

pub use config::PoolConfig;
pub use pool_controller::{
    start_pool_controller, PoolCommandSender, PoolManager,
};
pub use error::PoolError;

#[cfg(test)]
mod tests;
