// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! # Overview
//!
//! This crate provides all the facilities to interact with a running execution worker (massa-execution-worker crate)
//! that is in charge of executing operations containing bytecode in a virtual machine,
//! and applying the effects of the execution to a ledger.
//!
//! # Usage
//!
//! When an execution worker is launched to run in a separate worker thread for the whole duration of the process and
//! apply incoming requests, an instance of ExecutionManager is returned (see the documentation of massa-execution-worker).
//!
//! ExecutionManager allows stopping the execution worker thread,
//! but it also allows generating as many instances of ExecutionController as necessary.
//!
//! Each ExecutionController allows sending updates on the latest blockclique changes to the execution worker
//! for it to keep track of them and execute the bytecode present in blocks.
//! It also allows various read-only queries such as executing bytecode
//! while ignoring all the changes it would cause to the consensus state,
//! or reading the state at the output of the executed blockclique blocks.

mod config;
mod controller_traits;
mod error;
mod event_store;
mod types;

pub use config::ExecutionConfig;
pub use controller_traits::{ExecutionController, ExecutionManager};
pub use error::ExecutionError;
pub use event_store::EventStore;
pub use types::{ExecutionOutput, ExecutionStackElement, ReadOnlyExecutionRequest};

#[cfg(feature = "testing")]
pub mod test_exports;
