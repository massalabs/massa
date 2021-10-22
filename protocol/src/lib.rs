// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

mod config;
mod protocol_controller;
mod protocol_worker;

mod error;

pub use config::ProtocolConfig;
pub use error::ProtocolError;
pub use protocol_controller::{
    start_protocol_controller, ProtocolCommandSender, ProtocolEventReceiver, ProtocolManager,
    ProtocolPoolEventReceiver,
};
pub use protocol_worker::{ProtocolCommand, ProtocolEvent, ProtocolPoolEvent};

#[cfg(test)]
pub mod tests;
