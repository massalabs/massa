// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Protocol component
//! High level management of communications between nodes

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]
#![warn(missing_docs)]

mod protocol_worker;

pub use protocol_worker::start_protocol_controller;

#[cfg(test)]
pub mod tests;
