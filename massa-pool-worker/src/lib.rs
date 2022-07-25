// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Pool of operation and endorsements waiting to be included in a block
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_first_last)]

mod endorsement_pool;
mod operation_pool;
mod pool_worker;
mod controller_impl;
mod run;

pub use run::start_pool_controller;

#[cfg(test)]
mod tests;
