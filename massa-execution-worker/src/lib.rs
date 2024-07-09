// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # General description
//!
//! The execution worker launches a persistent thread allowing the execution
//! of operations that can contain executable bytecode and managing interactions with the ledger.
//! When the worker is launched, a `ExecutionManager` and a `ExecutionController` are returned.
//! `ExecutionManager` allows stopping the worker,
//! and `ExecutionController` is the clonable structure through which users interact with the worker.
//!
//! The worker is fed through the `ExecutionController` with information about blockclique changes and newly finalized blocks
//! and will execute the operations in those blocks, as well as pending asynchronous operations on empty slots.
//! The worker can also query the current state of the ledger, and simulate operations in a read-only context.
//!
//! The execution worker has shared read access to the final ledger,
//! and must be the only module with runtime write access to the final ledger.
//!
//! # A note on finality
//!
//!
//!
//! The operations contained in a final slot are ready to be executed as final
//! only once all the previous slots are final and their operations are executed as final or ready to be so.
//! This ensures the sequential order of the final executions of operations,
//! thus ensuring that writes to the final ledger are irreversible.
//!
//! Slots are called "active" if they have not been executed as final, and are not ready to be executed as final.
//! Active slots can therefore be final slots, or slots containing blocks from the blockclique, or empty (miss) slots.
//! Active slots can be executed in a speculative way: their execution might need to be reverted
//! as new blocks finalize or arrive, causing changes to them or to active slots before them.
//!
//! Miss slots are executed as well because they can contain implicit and asynchronous operations.
//!
//! # Architecture
//!
//! This crate is meant to be included only at the binary level to launch the worker,
//! not by the lib crates that will interact with it.
//! It depends on the massa-execution-exports crate that contains all the publicly exposed elements
//! and through which users will actually interact with the worker.
//!
//! ## worker.rs
//! This module runs the main loop of the worker thread.
//! It contains the logic to process incoming blockclique change notifications and read-only execution requests.
//! It sequences the blocks according to their slot number into queues,
//! and requests the execution of active and final slots to execution.rs.
//!
//! ## slot_sequencer.rs
//! Implements `SlotSequencer`
//! that allows sequencing slots for execution.
//!
//!
//! ## controller.rs
//! Implements `ExecutionManager` and `ExecutionController`
//! that serve as interfaces for users to interact with the worker in worker.rs.
//!
//! ## execution.rs
//! Contains the machinery to execute final and non-final slots,
//! and track the state and results of those executions.
//! This module initializes and holds a reference to the interface from `interface_impl.rs`
//! that allows the crate to provide execution state access
//! to the virtual machine runtime (massa-sc-runtime crate).
//! It also serves as an access point to the current execution state and speculative ledger
//! as defined in `speculative_ledger.rs`.
//!
//! ## `speculative_ledger.rs`
//! A speculative (non-final) ledger that supports canceling already-executed operations
//! in the case of some blockclique changes.
//!
//! ## `speculative_executed_ops.rs`
//! A speculative (non-final) list of previously executed operations to prevent reuse.
//!
//! ## `request_queue.rs`
//! This module contains the implementation of a generic finite-size execution request queue.
//! It handles requests that come with an MPSC to send back the result of their execution once it's done.
//!
//! ## `stats.rs`
//! Defines a structure that gathers execution statistics.

#![warn(missing_docs)]
#![warn(unused_crate_dependencies)]

mod active_history;
mod context;
mod controller;
mod execution;
mod interface_impl;
mod request_queue;
mod slot_sequencer;
mod speculative_async_pool;
mod speculative_deferred_calls;
mod speculative_executed_denunciations;
mod speculative_executed_ops;
mod speculative_ledger;
mod speculative_roll_state;
mod stats;
/// Provide abstraction and implementations of a storage backend for the the
/// dump-block feature
pub mod storage_backend;
mod worker;

#[cfg(feature = "execution-trace")]
mod trace_history;

mod execution_info;

use massa_db_exports as _;
pub use worker::start_execution_worker;

#[cfg(any(
    feature = "gas_calibration",
    feature = "benchmarking",
    feature = "test-exports"
))]
pub use interface_impl::InterfaceImpl;

#[cfg(feature = "benchmarking")]
use criterion as _;

#[cfg(test)]
mod tests;
