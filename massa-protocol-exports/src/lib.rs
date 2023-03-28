// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! protocol component exports
#![feature(async_closure)]
#![feature(drain_filter)]
#![feature(ip)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
mod channels;
mod error;
mod protocol_controller;
mod settings;

pub use channels::{ProtocolReceivers, ProtocolSenders};
pub use error::ProtocolError;
pub use protocol_controller::{
    BlocksResults, ProtocolCommand, ProtocolCommandSender, ProtocolManagementCommand,
    ProtocolManager,
};
pub use settings::ProtocolConfig;

/// TODO: Add only if test. Removed the configuration test because don't work if running cargo test on an other sub-crate.
pub mod tests;

/// Tests utils
#[cfg(feature = "testing")]
/// Exports testing tools as mock, functions helpers and configurations
pub mod test_exports;
