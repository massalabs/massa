#![feature(map_first_last)]
#![feature(unzip_option)]

mod config;
mod context;
mod controller;
mod error;
mod event_store;
mod execution;
mod exports;
mod interface_impl;
mod sce_ledger;
mod speculative_ledger;
mod types;
mod vm;
mod vm_thread;
mod worker;

pub use error::ExecutionError;
pub use exports::BootstrapExecutionState;
pub use sce_ledger::{SCELedger, SCELedgerEntry};
pub use worker::ExecutionCommand;
pub use worker::ExecutionEvent;
pub use worker::ExecutionManagementCommand;
pub use worker::ExecutionWorker;

#[cfg(test)]
mod tests;
