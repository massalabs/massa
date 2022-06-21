// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Protocol component
//! High level management of communications between nodes

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

/// protocol worker
pub mod protocol_worker;
pub mod worker_operations_impl;
pub use protocol_worker::start_protocol_controller;
mod checked_operations;
mod node_info;

#[cfg(test)]
pub mod tests;
