#![feature(map_first_last)]

mod controller;
mod error;
mod exports;
mod interface_impl;
mod sce_ledger;
mod settings;
mod types;
mod vm;
mod worker;

pub use controller::{
    start_controller, ExecutionCommandSender, ExecutionEventReceiver, ExecutionManager,
};
pub use error::ExecutionError;
pub use exports::BootstrapExecutionState;
pub use sce_ledger::{SCELedger, SCELedgerEntry};
pub use settings::{ExecutionConfigs, ExecutionSettings};
pub use worker::ExecutionCommand;
pub use worker::ExecutionEvent;
pub use worker::ExecutionManagementCommand;
pub use worker::ExecutionWorker;

#[cfg(test)]
mod tests;
