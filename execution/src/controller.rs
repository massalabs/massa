use crate::config::ExecutionConfig;
use crate::error::ExecutionError;

/// Commands sent to the `execution` component.
pub enum ExecutionCommand {}

/// A sender of execution commands.
#[derive(Clone)]
pub struct ExecutionCommandSender;

/// A sender of execution management commands.
pub struct ExecutionManager;

/// Creates a new execution controller.
///
/// # Arguments
/// * cfg: execution configuration
pub async fn start_consensus_controller(
    cfg: ExecutionConfig,
) -> Result<(ExecutionCommandSender, ExecutionManager), ExecutionError> {
    // Note: use async or threads?
    // Share ledger and clique with consensus?
    Ok((ExecutionCommandSender, ExecutionManager))
}
