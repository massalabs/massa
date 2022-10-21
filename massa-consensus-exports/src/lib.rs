// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Consensus exports
#![feature(async_closure)]
#![feature(hash_drain_filter)]
#![feature(int_roundings)]
#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

pub use consensus_controller::{ConsensusCommandSender, ConsensusEventReceiver, ConsensusManager};
pub use error::ConsensusError;
pub use settings::ConsensusConfig;

mod consensus_controller;

/// consensus errors
pub mod error;

/// consensus settings
pub mod settings;

/// consensus commands
pub mod commands;

/// consensus events
pub mod events;

/// consensus test tools
#[cfg(feature = "testing")]
pub mod test_exports;
