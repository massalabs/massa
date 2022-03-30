// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]

mod error;
mod protocol_controller;
mod settings;

pub use error::ProtocolError;
pub use protocol_controller::{
    BlocksResults, ProtocolCommand, ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver,
    ProtocolManagementCommand, ProtocolManager, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};
pub use settings::ProtocolSettings;

//TODO: Add only if test. Removed the cfg test because don't work if running cargo test on an other sub-crate.
pub mod tests;
