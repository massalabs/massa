use crate::config::ExecutionConfig;
use crate::error::ExecutionError;
use models::{Block, BlockHashMap};
use tokio::sync::mpsc;

/// Commands sent to the `execution` component.
pub enum ExecutionCommand {
    /// The clique has changed,
    /// contains the blocks of the new blockclique
    /// and a list of blocks that became final
    BlockCliqueChanged {
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    },
}

/// Management commands sent to the `execution` component.
pub enum ExecutionManagementCommand {}

pub struct ExecutionWorker {
    /// Configuration
    _cfg: ExecutionConfig,
    /// Receiver of commands.
    controller_command_rx: mpsc::Receiver<ExecutionCommand>,
    /// Receiver of management commands.
    controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
}

impl ExecutionWorker {
    pub async fn new(
        cfg: ExecutionConfig,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        let worker = ExecutionWorker {
            _cfg: cfg,
            controller_command_rx,
            controller_manager_rx,
        };

        // TODO: start a thread to run the actual VM?

        Ok(worker)
    }

    pub async fn run_loop(mut self) -> Result<(), ExecutionError> {
        loop {
            tokio::select! {
                // Process management commands
                cmd = self.controller_manager_rx.recv() => {
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}

                // Process commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd).await?,
            }
        }
        // end loop
        Ok(())
    }

    /// Process a given command.
    ///
    /// # Argument
    /// * cmd: command to process
    async fn process_command(&mut self, cmd: ExecutionCommand) -> Result<(), ExecutionError> {
        match cmd {
            ExecutionCommand::BlockCliqueChanged {
                blockclique,
                finalized_blocks,
            } => {
                self.blockclique_changed(blockclique, finalized_blocks)?;
            }
        }
        Ok(())
    }

    fn blockclique_changed(
        &mut self,
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        // TODO apply finalized blocks (note that they might not be SCE-final yet)

        // TODO apply new blockclique

        Ok(())
    }
}
