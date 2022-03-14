//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! This crate implements a
//!
//! # Architecture
//!
//! ## ledger.rs
//! Defines the FinalLedger that matches an address to a LedgerEntry (see ledger_entry.rs),
//! and can be manipulated using LedgerChanges (see ledger_changes.rs).
//! The FinalLedger is bootstrapped using tooling available in bootstrap.rs
//!
//!
//! ## Test exports
//!
//! When the crate feature `testing` is enabled, tooling useful for testing purposes is exported.
//! See test_exports/mod.rs for details.

#![feature(map_first_last)]
#![feature(async_closure)]

mod bootstrap;
mod config;
mod error;
mod final_state;
mod state_changes;

pub use bootstrap::FinalStateBootstrap;
pub use config::FinalStateConfig;
pub use error::FinalStateError;
pub use final_state::FinalState;
pub use state_changes::StateChanges;

#[cfg(test)]
mod tests;

#[cfg(feature = "testing")]
pub mod test_exports;
