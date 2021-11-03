use crate::config::{ExecutionConfig, CHANNEL_SIZE};
use crate::error::ExecutionError;
use crate::worker::{ExecutionCommand, ExecutionManagementCommand, ExecutionWorker};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A sender of execution commands.
#[derive(Clone)]
pub struct ExecutionCommandSender(pub mpsc::Sender<ExecutionCommand>);

/// A sender of execution management commands.
pub struct ExecutionManager {
    join_handle: JoinHandle<Result<(), ExecutionError>>,
    manager_tx: mpsc::Sender<ExecutionManagementCommand>,
}

impl ExecutionManager {
    pub async fn stop(self) -> Result<(), ExecutionError> {
        drop(self.manager_tx);

        // TODO: conversion to ExecutionError.
        if self.join_handle.await.is_err() {
            return Err(ExecutionError::Nothing);
        };

        Ok(())
    }
}

/// Creates a new execution controller.
///
/// # Arguments
/// * cfg: execution configuration
///
/// TODO: add a consensus command sender,
/// to be able to send the `TransferToConsensus` message.
pub async fn start_controller(
    cfg: ExecutionConfig,
) -> Result<(ExecutionCommandSender, ExecutionManager), ExecutionError> {
    let (command_tx, command_rx) = mpsc::channel::<ExecutionCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ExecutionManagementCommand>(1);

    let worker = ExecutionWorker::new(cfg, command_rx, manager_rx).await?;

    let join_handle = tokio::spawn(async move {
        match worker.run_loop().await {
            Err(err) => Err(err),
            Ok(v) => Ok(v),
        }
    });

    Ok((
        ExecutionCommandSender(command_tx),
        ExecutionManager {
            join_handle,
            manager_tx,
        },
    ))
}
