// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! This crate implements a ledger matching addresses to balances, executable bytecode and data.
//! It also provides tools to manipulate ledger entries.
//!
//! `FinalLedger` is used as part of `FinalState` that represents the latest final state of the node
//! (see `massa-final-state` crate for more details).
//!
//! `FinalLedger` representing a ledger at a given slot that was executed as final
//! (see the massa-execution-worker crate for details on execution).
//! Only the execution worker writes into the final ledger.
//!
//! # A note on parallel vs sequential balance
//!
//! The distinctions between the parallel and the sequential balance of a ledger entry are the following:
//! * the parallel balance can be credited or spent in any slot
//! * the sequential balance can be credited in any slot but only spent in slots form the address' thread
//! * block producers are credited fees from the sequential balance,
//!   and they can ensure that this balance will be available for their block simply
//!   by looking for sequential balance spending within the block's thread.
//!
//! # Architecture
//!
//! ## `ledger.rs`
//! Defines the `FinalLedger` that matches an address to a `LedgerEntry` (see `ledger_entry.rs`),
//! and can be manipulated using `LedgerChanges` (see `ledger_changes.rs`).
//! The `FinalLedger` is bootstrapped using tooling available in bootstrap.rs
//!
//! ## `ledger_entry.rs`
//! Represents an entry in the ledger for a given address.
//! It contains balances, executable bytecode and an arbitrary datastore.
//!
//! ## `ledger_changes.rs`
//! Represents a list of changes to ledger entries that
//! can be modified, combined or applied to the final ledger.
//!
//! ## `bootstrap.rs`
//! Provides serializable structures and tools for bootstrapping the final ledger.  
//!
//! ## Test exports
//!
//! When the crate feature `testing` is enabled, tooling useful for testing purposes is exported.
//! See `test_exports/mod.rs` for details.

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(map_first_last)]
#![feature(async_closure)]

mod bootstrap;
mod config;
mod error;
mod ledger;
mod ledger_changes;
mod ledger_entry;
mod types;

pub use bootstrap::FinalLedgerBootstrapState;
pub use config::LedgerConfig;
pub use error::LedgerError;
pub use ledger::FinalLedger;
pub use ledger_changes::LedgerChanges;
pub use ledger_entry::LedgerEntry;
pub use types::{Applicable, SetOrDelete, SetOrKeep, SetUpdateOrDelete};

#[cfg(test)]
mod tests;

#[cfg(feature = "testing")]
/// test exports
pub mod test_exports;
