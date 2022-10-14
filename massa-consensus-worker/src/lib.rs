//! Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(hash_drain_filter)]
#![feature(int_roundings)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#[macro_use]
extern crate massa_logging;

mod consensus_worker;
mod tools;
pub use tools::start_consensus_controller;

#[cfg(test)]
mod tests;
