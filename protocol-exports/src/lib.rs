// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

mod error;
mod protocol_controller;
mod settings;

pub use error::ProtocolError;
pub use protocol_controller::{
    ProtocolCommand, ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver,
    ProtocolManagementCommand, ProtocolManager, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};
pub use settings::{ProtocolSettings, CHANNEL_SIZE};

pub mod tests;
