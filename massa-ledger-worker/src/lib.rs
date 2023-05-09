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

mod ledger;
mod ledger_db;

pub(crate)  use ledger::FinalLedger;

#[cfg(test)]
mod tests;

#[cfg(feature = "testing")]
pub(crate)  mod test_exports;
