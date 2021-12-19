use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::error::ExecutionError;
use crate::types::{ExecutionQueue, ExecutionRequest};
use crate::vm::VM;
use crate::BootstrapExecutionState;
use crate::{config::ExecutionSettings, types::ExecutionStep};
use massa_models::timeslots::{get_block_slot_timestamp, get_current_latest_block_slot};
use massa_models::{Block, BlockHashMap, BlockId, Slot};
use tokio::sync::mpsc;
use tokio::time::sleep_until;
use tracing::{debug, warn};

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

    /// Get a snapshot of the current state for bootstrap
    GetBootstrapState(tokio::sync::oneshot::Sender<BootstrapExecutionState>),
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
    cfg: ExecutionSettings,
    /// VM
    vm: Arc<Mutex<VM>>,
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
    /// VM thread
    vm_thread: JoinHandle<()>,
    /// VM execution requests queue
    execution_queue: ExecutionQueue,
}

impl ExecutionWorker {
    pub fn new(
        cfg: ExecutionSettings,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
        bootstrap_state: Option<BootstrapExecutionState>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        let execution_queue = ExecutionQueue::default();
        let execution_queue_clone = execution_queue.clone();

        // Check bootstrap
        let bootstrap_final_slot;
        let bootstrap_ledger;
        if let Some(bootstrap_state) = bootstrap_state {
            // init from bootstrap
            bootstrap_final_slot = bootstrap_state.final_slot;
            bootstrap_ledger = Some((bootstrap_state.final_ledger, bootstrap_final_slot));
        } else {
            // init without bootstrap
            bootstrap_final_slot = Slot::new(0, cfg.thread_count.saturating_sub(1));
            bootstrap_ledger = None;
        };

        // Init VM
        let vm = Arc::new(Mutex::new(VM::new(cfg.clone(), bootstrap_ledger)?));
        let vm_clone = vm.clone();

        // Start VM thread
        let vm_thread = thread::spawn(move || {
            let (lock, condvar) = &*execution_queue_clone;
            let mut requests = lock.lock().unwrap();
            // Run until shutdown.
            loop {
                match &requests.pop_front() {
                    Some(ExecutionRequest::RunFinalStep(step)) => {
                        vm_clone.lock().unwrap().run_final_step(step)
                    }
                    Some(ExecutionRequest::RunActiveStep(step)) => {
                        vm_clone.lock().unwrap().run_active_step(step)
                    }
                    Some(ExecutionRequest::ResetToFinalState) => {
                        vm_clone.lock().unwrap().reset_to_final()
                    }
                    Some(ExecutionRequest::Shutdown) => return,
                    None => { /* startup or spurious wakeup */ }
                };
                requests = condvar.wait(requests).unwrap();
            }
        });

        // return execution worker
        Ok(ExecutionWorker {
            cfg,
            vm,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
            //TODO bootstrap or init
            last_final_slot: bootstrap_final_slot,
            last_active_slot: bootstrap_final_slot,
            ordered_active_blocks: Default::default(),
            ordered_pending_css_final_blocks: Default::default(),
            vm_thread,
            execution_queue,
        })
    }

    // asks the VM to reset to its final
    pub fn reset_to_final(&mut self) {
        let (queue_lock, condvar) = &*self.execution_queue;
        let queue_guard = &mut queue_lock.lock().unwrap();
        // cancel all non-final requests
        // Final execution requests are left to maintain final state consistency
        queue_guard.retain(|req| {
            matches!(
                req,
                ExecutionRequest::RunFinalStep(..) | ExecutionRequest::Shutdown
            )
        });
        // request reset to final state
        queue_guard.push_back(ExecutionRequest::ResetToFinalState);
        // notify
        condvar.notify_one();
    }

    /// runs an SCE-active step (slot)
    ///
    /// # Arguments
    /// * slot: target slot
    /// * block: None if miss, Some(block_id, block) otherwise
    fn push_request(&mut self, request: ExecutionRequest) {
        let (queue_lock, condvar) = &*self.execution_queue;
        let queue_guard = &mut queue_lock.lock().unwrap();
        queue_guard.push_back(request);
        condvar.notify_one();
    }

    fn get_timer_to_next_slot(&self) -> Result<tokio::time::Sleep, ExecutionError> {
        Ok(sleep_until(
            get_block_slot_timestamp(
                self.cfg.thread_count,
                self.cfg.t0,
                self.cfg.genesis_timestamp,
                get_current_latest_block_slot(
                    self.cfg.thread_count,
                    self.cfg.t0,
                    self.cfg.genesis_timestamp,
                    self.cfg.clock_compensation,
                )?
                .map_or(Ok(Slot::new(0, 0)), |v| {
                    v.get_next_slot(self.cfg.thread_count)
                })?,
            )?
            .estimate_instant(self.cfg.clock_compensation)?,
        ))
    }

    pub async fn run_loop(mut self) -> Result<(), ExecutionError> {
        // set slot timer
        let next_slot_timer = self.get_timer_to_next_slot()?;
        tokio::pin!(next_slot_timer);
        loop {
            tokio::select! {
                // Process management commands
                _ = self.controller_manager_rx.recv() => break,
                // Process commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd)?,
                // Process slot timer event
                _ = &mut next_slot_timer => {
                    self.fill_misses_until_now()?;
                    next_slot_timer.set(self.get_timer_to_next_slot()?);
                }
            }
        }
        // Shutdown VM, cancel all pending execution requests
        self.push_request(ExecutionRequest::Shutdown);
        if self.vm_thread.join().is_err() {
            debug!("Failed joining vm thread")
        }
        Ok(())
    }

    /// Proces a given command.
    ///
    /// # Argument
    /// * cmd: command to process
    fn process_command(&mut self, cmd: ExecutionCommand) -> Result<(), ExecutionError> {
        match cmd {
            ExecutionCommand::BlockCliqueChanged {
                blockclique,
                finalized_blocks,
            } => {
                self.blockclique_changed(blockclique, finalized_blocks)?;
            }

            ExecutionCommand::GetBootstrapState(response_tx) => {
                let (vm_ledger, vm_slot) = self.vm.lock().unwrap().get_bootstrap_state();
                let bootstrap_state = BootstrapExecutionState {
                    final_ledger: vm_ledger,
                    final_slot: vm_slot,
                };
                if response_tx.send(bootstrap_state).is_err() {
                    warn!("execution: could not send get_bootstrap_state answer");
                }
            }
        }
        Ok(())
    }

    /// fills the remaining slots until now() with miss executions
    fn fill_misses_until_now(&mut self) -> Result<(), ExecutionError> {
        let end_step = get_current_latest_block_slot(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            self.cfg.clock_compensation,
        )?;
        if let Some(end_step) = end_step {
            while self.last_active_slot < end_step {
                self.last_active_slot =
                    self.last_active_slot.get_next_slot(self.cfg.thread_count)?;
                self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                    slot: self.last_active_slot,
                    block: None,
                }));
            }
        }
        Ok(())
    }

    /// called when the blockclique changes
    fn blockclique_changed(
        &mut self,
        mut blockclique: BlockHashMap<Block>,
        mut finalized_blocks: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        // filter out any already-finalized blocks
        // this can happen for example on bootstrap because the consensus vs execution bootstrap snapshots may be out of sync
        blockclique.retain(|_b_id, b| b.header.content.slot > self.last_final_slot);
        finalized_blocks.retain(|_b_id, b| b.header.content.slot > self.last_final_slot);

        // stop the current VM execution and reset state to final
        // TODO make something more iterative/conservative in the future to reuse unaffected executions
        self.reset_to_final();

        // stop the current VM execution and reset state to final
        // TODO reset VM

        // gather pending finalized CSS
        let mut css_final_blocks: Vec<(BlockId, Block)> = self
            .ordered_pending_css_final_blocks
            .drain(..)
            .chain(finalized_blocks.into_iter())
            .collect();
        css_final_blocks.sort_unstable_by_key(|(_, b)| b.header.content.slot);

        // list maximum thread slots
        let mut max_thread_slot = vec![self.last_final_slot; self.cfg.thread_count as usize];
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
                let next_final_slot = self.last_final_slot.get_next_slot(self.cfg.thread_count)?;
                if next_final_slot == block_slot {
                    self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                        slot: next_final_slot,
                        block: Some((b_id, block.clone())),
                    }));
                    self.last_final_slot = next_final_slot;
                    break;
                } else if next_final_slot < max_thread_slot[next_final_slot.thread as usize] {
                    self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                        slot: next_final_slot,
                        block: Some((b_id, block.clone())),
                    }));
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
        self.last_active_slot = self.last_final_slot;
        // TODO remove clone() in iterator below
        for (b_id, block) in self.ordered_active_blocks.clone() {
            // process misses
            if self.last_active_slot == self.last_final_slot {
                self.last_active_slot =
                    self.last_active_slot.get_next_slot(self.cfg.thread_count)?;
            }
            while self.last_active_slot < block.header.content.slot {
                self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                    slot: self.last_active_slot,
                    block: None,
                }));
                self.last_active_slot =
                    self.last_active_slot.get_next_slot(self.cfg.thread_count)?;
            }
            self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                slot: self.last_active_slot,
                block: Some((b_id, block)),
            }));
        }

        // apply misses until now()
        self.fill_misses_until_now()?;

        Ok(())
    }
}
