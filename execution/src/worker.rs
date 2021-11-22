use crate::config::ExecutionConfig;
use crate::error::ExecutionError;
use models::{address::AddressHashMap, Amount, Block, BlockHashMap, BlockId, Slot};
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

// Events produced by the execution component.
pub enum ExecutionEvent {
    /// A coin transfer
    /// from the SCE ledger to the CSS ledger.
    TransferToConsensus,
}

/// Management commands sent to the `execution` component.
pub enum ExecutionManagementCommand {}

struct SCELedgerEntry {
    balance: Amount,
    bytecode: Vec<u8>,
    data: Vec<u8>,
}

pub struct ExecutionWorker {
    /// Configuration
    _cfg: ExecutionConfig,
    /// Thread count
    thread_count: u8,
    /// Receiver of commands.
    controller_command_rx: mpsc::Receiver<ExecutionCommand>,
    /// Receiver of management commands.
    controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    /// Sender of events.
    _event_sender: mpsc::UnboundedSender<ExecutionEvent>,
    /// Time cursors
    last_final_slot: Slot,
    last_active_slot: Slot,
    /// SCE ledger
    final_ledger: AddressHashMap<SCELedgerEntry>,
    active_ledger: AddressHashMap<SCELedgerEntry>,
    /// ordered active blocks
    ordered_active_blocks: Vec<(BlockId, Block)>,
    /// pending CSS final blocks
    ordered_pending_css_final_blocks: Vec<(BlockId, Block)>,
}

impl ExecutionWorker {
    pub async fn new(
        cfg: ExecutionConfig,
        thread_count: u8,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        let worker = ExecutionWorker {
            _cfg: cfg,
            thread_count,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
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
        // gather pending finalized CSS
        let mut css_final_blocks: Vec<(BlockId, Block)> = self
            .ordered_pending_css_final_blocks
            .into_iter()
            .chain(finalized_blocks.into_iter())
            .collect();
        css_final_blocks.sort_unstable_by_key(|(_, b)| b.header.content.slot);

        // list maximum thread slots
        let mut max_thread_slot = vec![self.last_final_slot; self.thread_count as usize];
        for (_b_id, block) in css_final_blocks.iter() {
            max_thread_slot[block.header.content.slot.thread as usize] = std::cmp::max(
                max_thread_slot[block.header.content.slot.thread as usize],
                block.header.content.slot,
            );
        }

        // list final blocks
        for (b_id, block) in css_final_blocks.into_iter() {
            let block_slot = block.header.content.slot;
            if block_slot <= self.last_final_slot {
                continue;
            }

            loop {
                let next_final_slot = self.last_final_slot.get_next_slot(self.thread_count)?;
                if block_slot == next_final_slot {
                    //TODO apply block to self.final_ledger
                } else if next_final_slot < max_thread_slot[next_final_slot.thread as usize] {
                    //TODO apply miss to self.final_ledger
                } else {
                    self.ordered_pending_css_final_blocks.push((b_id, block));
                    break;
                }
                self.last_final_slot = next_final_slot;
            }
        }

        // apply remaining CSS finals + new blockclique to obtain candidate state
        self.ordered_active_blocks = self
            .ordered_pending_css_final_blocks
            .iter()
            .cloned()
            .chain(
                blockclique
                    .into_iter()
                    .filter(|(_b_id, b)| b.header.content.slot > self.last_final_slot),
            )
            .collect();
        self.ordered_active_blocks
            .sort_unstable_by_key(|(_b_id, b)| b.header.content.slot);
        // TODO apply self.ordered_active_blocks to self.final_ledger to obtain self.candidate_ledger

        Ok(())
    }
}
