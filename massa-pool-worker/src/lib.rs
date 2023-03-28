//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Pool of operation and endorsements waiting to be included in a block

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(async_closure)]
#![feature(map_try_insert)]
#![feature(let_chains)]

mod controller_impl;
mod denunciation_pool;
mod endorsement_pool;
mod operation_pool;
mod types;
mod worker;

pub use worker::start_pool_controller;

#[cfg(test)]
mod tests;
