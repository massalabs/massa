use crate::config::ExecutionConfig;
use crate::error::ExecutionError;
use models::{Block, BlockHashMap, BlockId, Slot};

use crate::vm::{ExecutionStep, VM};
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
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

/// execution request
enum ExecutionRequest {
    RunFinalStep(ExecutionStep),  // Runs a final step
    RunActiveStep(ExecutionStep), // Runs an active step
    ResetToFinalState,            // Resets the VM to its final state
    Stop,                         // Stops the VM thread
}

pub struct ExecutionWorker {
    /// Configuration
    cfg: ExecutionConfig,
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
    /// Execution queue.
    execution_queue: Arc<(Mutex<VecDeque<ExecutionRequest>>, Condvar)>,
    /// VM thread join handle.
    vm_join_handle: JoinHandle<()>,
}

impl ExecutionWorker {
    pub async fn new(
        cfg: ExecutionConfig,
        thread_count: u8,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        // setup execution request queue
        let execution_queue: Arc<(Mutex<VecDeque<ExecutionRequest>>, Condvar)> =
            Arc::new((Mutex::new(Default::default()), Condvar::new()));
        let execution_queue_clone = Arc::clone(&execution_queue);

        // launch VM thread
        let cfg_clone = cfg.clone();
        let vm_join_handle = thread::spawn(move || {
            // init VM
            let mut vm = VM::new(cfg_clone);

            // handle execution requests
            let (queue_lock, condvar) = &*execution_queue_clone;
            let mut queue_guard = queue_lock.lock();
            loop {
                condvar.wait(&mut queue_guard);
                match (*queue_guard).pop_front() {
                    Some(ExecutionRequest::Stop) => {
                        break;
                    }
                    Some(ExecutionRequest::ResetToFinalState) => {
                        vm.reset_to_final();
                    }
                    Some(ExecutionRequest::RunFinalStep(step)) => {
                        vm.run_final_step(step);
                    }
                    Some(ExecutionRequest::RunActiveStep(step)) => {
                        vm.run_active_step(step);
                    }
                    None => {}
                }
            }
        });

        // return execution worker
        Ok(ExecutionWorker {
            cfg,
            thread_count,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
            //TODO bootstrap or init
            last_final_slot: Slot::new(0, 0),
            last_active_slot: Slot::new(0, 0),
            ordered_active_blocks: Default::default(),
            ordered_pending_css_final_blocks: Default::default(),
            execution_queue,
            vm_join_handle,
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

        // Shutdown VM, cancel all pending execution requests
        {
            let (queue_lock, condvar) = &*self.execution_queue;
            let mut queue_guard = queue_lock.lock();
            (*queue_guard).push_front(ExecutionRequest::Stop);
            condvar.notify_all();
        }
        let _ = self.vm_join_handle.join();

        Ok(())
    }

    // asks the VM to reset to its final
    fn vm_reset(&mut self) {
        let (queue_lock, condvar) = &*self.execution_queue;
        let mut queue_guard = queue_lock.lock();
        // cancel all non-final, non-stop requests
        // Final execution requests are left to maintain final state consistency
        (*queue_guard).retain(|req| match req {
            ExecutionRequest::RunFinalStep(..) => true,
            ExecutionRequest::Stop => true,
            ExecutionRequest::RunActiveStep(..) => false,
            ExecutionRequest::ResetToFinalState => false,
        });
        // request reset to final state
        (*queue_guard).push_back(ExecutionRequest::ResetToFinalState);
        // notify
        condvar.notify_one();
    }

    /// runs an SCE-final step (slot)
    ///
    /// # Arguments
    /// * slot: target slot
    /// * block: None if miss, Some(block_id, block) otherwise
    fn vm_run_final_step(&mut self, slot: Slot, block: Option<(BlockId, Block)>) {
        let (queue_lock, condvar) = &*self.execution_queue;
        let mut queue_guard = queue_lock.lock();
        (*queue_guard).push_back(ExecutionRequest::RunFinalStep(ExecutionStep {
            slot,
            block,
        }));
        // notify
        condvar.notify_one();
    }

    /// runs an SCE-active step (slot)
    ///
    /// # Arguments
    /// * slot: target slot
    /// * block: None if miss, Some(block_id, block) otherwise
    fn vm_run_active_step(&mut self, slot: Slot, block: Option<(BlockId, Block)>) {
        let (queue_lock, condvar) = &*self.execution_queue;
        let mut queue_guard = queue_lock.lock();
        (*queue_guard).push_back(ExecutionRequest::RunActiveStep(ExecutionStep {
            slot,
            block,
        }));
        // notify
        condvar.notify_one();
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
        // stop the current VM execution and reset state to final
        self.vm_reset();

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
                    self.vm_run_final_step(next_final_slot, Some((b_id, block)));
                    self.last_final_slot = next_final_slot;
                    break;
                } else if next_final_slot < max_thread_slot[next_final_slot.thread as usize] {
                    self.vm_run_final_step(next_final_slot, None);
                    self.last_final_slot = next_final_slot;
                } else {
                    self.ordered_pending_css_final_blocks.push((b_id, block));
                    break;
                }
            }
        }

        // new blocks
        let new_blocks: Vec<(BlockId, Block)> = blockclique
            .into_iter()
            .filter(|(_b_id, b)| b.header.content.slot > self.last_final_slot)
            .collect();

        // list remaining CSS finals + new blockclique
        self.ordered_active_blocks = self
            .ordered_pending_css_final_blocks
            .iter()
            .cloned()
            .chain(new_blocks.clone().into_iter())
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
                self.vm_run_active_step(self.last_active_slot, None);
                self.last_active_slot = self.last_active_slot.get_next_slot(self.thread_count)?;
            }
            self.vm_run_active_step(self.last_active_slot, Some((b_id, block)));
        }
        Ok(())
    }
}
