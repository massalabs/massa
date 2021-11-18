mod config;
mod controller;
mod error;
mod worker;

pub use config::ExecutionConfig;

pub use controller::{
    start_controller, ExecutionCommandSender, ExecutionEventReceiver, ExecutionManager,
};
pub use error::ExecutionError;

#[cfg(test)]
mod tests;
