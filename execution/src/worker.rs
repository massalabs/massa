use crate::config::ExecutionConfig;
use crate::error::ExecutionError;
use models::{Block, BlockHashMap};
use tokio::sync::mpsc;

/// Commands sent to the `execution` component.
pub enum ExecutionCommand {
    /// The clique has changed,
    /// contains the blocks of the current clique.
    BlockCliqueChanged(BlockHashMap<Block>),
    /// Some blocks have become final.
    BlocksFinal(BlockHashMap<Block>),
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
            ExecutionCommand::BlockCliqueChanged(_new_clique) => {
                // Spec: https://github.com/massalabs/massa/docs/execution.md#scecss-pipeline
                //
                // TODO: https://github.com/massalabs/massa/issues/1800
                //
                // 1. reset the Candidate ledger to the Final ledger
                // 2.1 sort all blockclique blocks by increasing slot
                // for each block B in that order,
                // the SCE processes the ExecuteSC operations in the block in the order they appear in the block.
                //
                // For each such operation:
                // 3.1 credit the block creator in the SCE ledger with op.max_gas * op.gas_price
                // 3.2 execute the smart contract bytecode (rollback and ignore in case of failure, but to not reimburse gas fees
            }
            ExecutionCommand::BlocksFinal(_new_final_blocks) => {
                // Spec: https://github.com/massalabs/massa/docs/execution.md#scecss-pipeline
                //
                // TODO: https://github.com/massalabs/massa/issues/1801
                //
                // 1. Determine if, based on those new final blocks as per consensus,
                // there are any new blocks final from an execution perspective.
                //
                // Note: a slot is SCE-final if its immediate predecessor slot contains a block that is CSS-final
                // or is empty but followed in its own thread by a CSS-final block.
                //
                // 2. For each new final block in execution:
                // 2.1 update the Final SCE ledger by VM-executing the newly SCE-final block on top of the current Final SCE ledger .
                // 2.2 if the block B requires transferring coins from the SCE ledger to the CSS ledger,
                //     send a TransferToConsensus to the CSS
            }
        }
        Ok(())
    }
}
