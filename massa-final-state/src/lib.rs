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
//!
//! # Network restart documentation
//!
//! ## Goals of the network restart
//! If the blockchain crashes (corrupted / attacked ledger, all nodes crash, etc.) and we want to keep the same main parameters of the network (same `GENESIS_TIMESTAMP`, same ledger, same final_state, etc.), then we can restart the network.
//!
//! **ONE** node should restart from a snapshot (which is just the RocksDB ledger, read as usual), and the other nodes should bootstrap from it.
//!
//! ## Command line
//!
//! ```sh
//! cargo run --release -- --restart-from-snapshot-at-period 200
//! ```
//!
//! Means: the node will restart from the ledger and final_state on disk (usual path in the config). Block production will start once the period given in args is reached (here, 200).
//!
//! ## Scenario
//!
//! 1. At period 40, the network crashes.
//! 2. We restart one node N0, at the time of period 80, with `cargo run --release -- --restart-from-snapshot-at-period 200`
//! 3. We start one other node N1, at the time of period 100, with `cargo run --release`
//! 4. The node N1 will bootstrap from N0. No blocks are produced yet.
//! 5. At the time of period 200, block production starts again.
//!
//! ## Additional notes
//!
//! ### Why is block production delayed?
//!
//! In order to give time to all nodes to rejoin the network after a crash and bootstrap. If we don't give them the time, their rolls would be sold because most stakers would have a lot of block miss.
//!
//! ### In sandbox
//!
//! Sandbox feature can be enabled. For instance, here is a test scenario:
//!
//! 1. Run the node as usual: `cargo run --release --features sandbox`
//! 2. Make transaction, buy rolls, etc.
//! 3. Shut down the node at slot S_0.
//! 4. Restart the network: `cargo run --release --features sandbox --restart-from-snapshot-at-period S_1`
//!
//! Here, the network will restart, and the network will start producing blocks again 10 seconds after launch.
//!
//! **/!\ This means that the genesis timestamp will be different between runs, but it should not matter in most cases.**
//!
//! ### Backups
//!
//! By default, the network restarts from the state associated with the last final slot before the shutdown.
//! However, we may sometimes want to recover from an earlier state (e.g. if an attacker stole 50% of all Massa, we want to restart with the state before the attack.
//! We use RocksDB checkpoint system to save the state at regular interval (see the `PERIODS_BETWEEN_BACKUPS` constant in `massa > massa-models > src > config > constants.rs`)
//! Backups for `Slot {period, thread}` are stored in `massa > massa-node > storage > ledger > rocks_db_backup > backup_[period]_[thread]`
//! Backups are hard links of the rocks_db, so the overhead of storing them should be minimal.
//! To recover from a backup, simply replace the contents of the rocks_db folder by the contents of the target backup folder.

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]
#![feature(hash_drain_filter)]
#![feature(async_closure)]
#![feature(map_try_insert)]
#![feature(let_chains)]

mod config;
mod error;
mod final_state;
mod mapping_grpc;
mod state_changes;

pub use config::FinalStateConfig;
pub use error::FinalStateError;
pub use final_state::FinalState;
pub use state_changes::{StateChanges, StateChangesDeserializer, StateChangesSerializer};

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "testing"))]
pub mod test_exports;
