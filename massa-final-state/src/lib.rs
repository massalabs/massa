//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! This crate implements a final state that encompasses a final ledger and asynchronous message pool.
//! Nodes store only one copy of this final state which is very large
//! (the copy is attached to the output of the last executed final slot),
//! and apply speculative changes on it to deduce its value at a non-final slot
//! (see `massa-execution-exports` crate for more details).
//! Nodes joining the network need to bootstrap this state.
//!
//! # Architecture
//!
//! ## `final_state.rs`
//! Defines the `FinalState` that matches that represents the state of the node at
//! the latest executed final slot. It contains the final ledger and the asynchronous event pool.
//! It can be manipulated using `StateChanges` (see `state_changes.rs`).
//! The `FinalState` is bootstrapped using tooling available in bootstrap.rs
//!
//! ## `state_changes.rs`
//! Represents a list of changes the final state.
//! It can be modified, combined or applied to the final ledger.
//!
//! ## `executed_ops.rs`
//! Defines a structure to list and prune previously executed operations.
//! Used to detect operation reuse.
//!
//! ## `bootstrap.rs`
//! Provides serializable structures and tools for bootstrapping the final state.
//!
//! ## Test exports
//!
//! When the crate feature `testing` is enabled, tooling useful for testing purposes is exported.
//! See `test_exports/mod.rs` for details.

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(hash_drain_filter)]
#![feature(async_closure)]
#![feature(map_try_insert)]

mod config;
mod error;
mod final_state;
mod state_changes;

pub(crate) use config::FinalStateConfig;
pub(crate) use error::FinalStateError;
pub use final_state::FinalState;
pub use state_changes::StateChanges;
pub(crate) use state_changes::StateChangesDeserializer;
pub(crate) use state_changes::StateChangesSerializer;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testing"))]
pub(crate) mod test_exports;
