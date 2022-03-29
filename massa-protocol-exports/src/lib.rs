// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! protocol component exports
#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
mod error;
mod protocol_controller;
mod settings;

pub use error::ProtocolError;
pub use protocol_controller::{
    ProtocolCommand, ProtocolCommandSender, ProtocolEvent, ProtocolEventReceiver,
    ProtocolManagementCommand, ProtocolManager, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};
pub use settings::ProtocolSettings;

/// test exports
pub mod tests;
