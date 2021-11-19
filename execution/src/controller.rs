use crate::config::{ExecutionConfig, CHANNEL_SIZE};
use crate::error::ExecutionError;
use crate::worker::{
    ExecutionCommand, ExecutionEvent, ExecutionManagementCommand, ExecutionWorker,
};
use models::{Block, BlockHashMap};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// A sender of execution commands.
#[derive(Clone)]
pub struct ExecutionCommandSender(pub mpsc::Sender<ExecutionCommand>);

/// A receiver of execution events.
pub struct ExecutionEventReceiver(pub mpsc::UnboundedReceiver<ExecutionEvent>);

/// A sender of execution management commands.
pub struct ExecutionManager {
    join_handle: JoinHandle<Result<(), ExecutionError>>,
    manager_tx: mpsc::Sender<ExecutionManagementCommand>,
}

impl ExecutionManager {
    pub async fn stop(self) -> Result<(), ExecutionError> {
        drop(self.manager_tx);

        if self.join_handle.await.is_err() {
            return Err(ExecutionError::JoinError);
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
) -> Result<
    (
        ExecutionCommandSender,
        ExecutionEventReceiver,
        ExecutionManager,
    ),
    ExecutionError,
> {
    let (command_tx, command_rx) = mpsc::channel::<ExecutionCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ExecutionManagementCommand>(1);

    // Unbounded, as execution is limited per metering already.
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ExecutionEvent>();

    let worker = ExecutionWorker::new(cfg, event_tx, command_rx, manager_rx).await?;

    let join_handle = tokio::spawn(async move {
        match worker.run_loop().await {
            Err(err) => Err(err),
            Ok(v) => Ok(v),
        }
    });

    Ok((
        ExecutionCommandSender(command_tx),
        ExecutionEventReceiver(event_rx),
        ExecutionManager {
            join_handle,
            manager_tx,
        },
    ))
}

impl ExecutionCommandSender {
    /// notify of a blockclique change
    pub async fn update_blockclique(
        &mut self,
        finalized_blocks: BlockHashMap<Block>,
        blockclique: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        self.0
            .send(ExecutionCommand::BlockCliqueChanged {
                blockclique,
                finalized_blocks,
            })
            .await
            .map_err(|_err| {
                ExecutionError::ChannelError(
                    "could not send BlockCliqueChanged command to execution".into(),
                )
            })?;
        Ok(())
    }
}
