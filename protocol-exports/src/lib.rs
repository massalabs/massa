// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

mod config;
mod error;
mod protocol_controller;

pub use config::{ProtocolConfig, CHANNEL_SIZE};
pub use error::ProtocolError;
pub use protocol_controller::{
    ProtocolCommand, ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver,
    ProtocolManagementCommand, ProtocolManager, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};

pub mod tests;
