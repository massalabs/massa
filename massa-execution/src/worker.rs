use crate::error::ExecutionError;
use crate::vm::VM;
use crate::{config::ExecutionConfig, types::ExecutionStep};
use massa_models::{Block, BlockHashMap, BlockId, Slot};
use tokio::sync::mpsc;

/// Commands sent to the `execution` component.
#[derive(Debug)]
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
    /// ordered active blocks
    ordered_active_blocks: Vec<(BlockId, Block)>,
    /// pending CSS final blocks
    ordered_pending_css_final_blocks: Vec<(BlockId, Block)>,
    /// VM
    vm: VM,
}

impl ExecutionWorker {
    pub async fn new(
        _cfg: ExecutionConfig,
        thread_count: u8,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        // return execution worker
        Ok(ExecutionWorker {
            _cfg: _cfg.clone(),
            thread_count,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
            //TODO bootstrap or init
            last_final_slot: Slot::new(0, 0),
            last_active_slot: Slot::new(0, 0),
            ordered_active_blocks: Default::default(),
            ordered_pending_css_final_blocks: Default::default(),
            vm: VM::new(_cfg),
        })
    }

    pub async fn run_loop(mut self) -> Result<(), ExecutionError> {
        loop {
            tokio::select! {
                // Process management commands
                cmd = self.controller_manager_rx.recv() => {
                    match cmd {
                        None => break,
                        Some(_) => {}
                    }
                },
                // Process commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd).await?,
            }
        }
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
                self.blockclique_changed(blockclique, finalized_blocks)
                    .await?;
            }
        }
        Ok(())
    }

    async fn blockclique_changed(
        &mut self,
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        // stop the current VM execution and reset state to final
        // TODO make something more iterative/conservative in the future to reuse unaffected executions

        // gather pending finalized CSS
        let mut css_final_blocks: Vec<(BlockId, Block)> = self
            .ordered_pending_css_final_blocks
            .drain(..)
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

        // list SCE-final slots/blocks
        for (b_id, block) in css_final_blocks.into_iter() {
            let block_slot = block.header.content.slot;
            if block_slot <= self.last_final_slot {
                continue;
            }
            loop {
                let next_final_slot = self.last_final_slot.get_next_slot(self.thread_count)?;
                if next_final_slot == block_slot {
                    self.vm
                        .run_final_step(&ExecutionStep {
                            slot: next_final_slot,
                            block: Some((b_id, block.clone())),
                        })
                        .await;
                    self.last_final_slot = next_final_slot;
                    break;
                } else if next_final_slot < max_thread_slot[next_final_slot.thread as usize] {
                    self.vm
                        .run_final_step(&ExecutionStep {
                            slot: next_final_slot,
                            block: Some((b_id, block.clone())),
                        })
                        .await;
                    self.last_final_slot = next_final_slot;
                } else {
                    self.ordered_pending_css_final_blocks.push((b_id, block));
                    break;
                }
            }
        }

        // list remaining CSS finals + new blockclique
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

        // sort active blocks
        self.ordered_active_blocks
            .sort_unstable_by_key(|(_b_id, b)| b.header.content.slot);

        // apply active blocks and misses
        // TODO remove clone() in iterator below
        for (b_id, block) in self.ordered_active_blocks.clone() {
            // process misses
            if self.last_active_slot == self.last_final_slot {
                self.last_active_slot = self.last_active_slot.get_next_slot(self.thread_count)?;
            }
            while self.last_active_slot < block.header.content.slot {
                self.vm
                    .run_active_step(&ExecutionStep {
                        slot: self.last_active_slot,
                        block: None,
                    })
                    .await;
                self.last_active_slot = self.last_active_slot.get_next_slot(self.thread_count)?;
            }
            self.vm
                .run_active_step(&ExecutionStep {
                    slot: self.last_active_slot,
                    block: Some((b_id, block)),
                })
                .await;
        }
        Ok(())
    }
}
