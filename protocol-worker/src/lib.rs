// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![doc = include_str!("../../docs/concurrency.md")]
#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

mod protocol_controller;

pub use protocol_controller::start_protocol_controller;

#[cfg(test)]
pub mod tests;
