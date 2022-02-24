// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

mod protocol_worker;

pub use protocol_worker::start_protocol_controller;

#[cfg(test)]
pub mod tests;
