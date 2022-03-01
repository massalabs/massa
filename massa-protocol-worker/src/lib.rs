// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

pub mod protocol_worker;
pub use protocol_worker::start_protocol_controller;
mod node_info;

#[cfg(test)]
pub mod tests;
