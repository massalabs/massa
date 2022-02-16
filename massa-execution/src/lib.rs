#![feature(map_first_last)]

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

pub use config::{ExecutionConfigs, ExecutionSettings};
pub use controller::{
    start_controller, ExecutionCommandSender, ExecutionEventReceiver, ExecutionManager,
};
pub use error::ExecutionError;
pub use exports::BootstrapExecutionState;
pub use sce_ledger::{SCELedger, SCELedgerEntry};
pub use worker::ExecutionCommand;
pub use worker::ExecutionEvent;
pub use worker::ExecutionManagementCommand;
pub use worker::ExecutionWorker;

#[cfg(test)]
mod tests;
