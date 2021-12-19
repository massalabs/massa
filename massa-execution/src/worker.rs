use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use crate::error::ExecutionError;
use crate::types::{ExecutionQueue, ExecutionRequest};
use crate::vm::VM;
use crate::BootstrapExecutionState;
use crate::{config::ExecutionSettings, types::ExecutionStep};
use massa_models::{Block, BlockHashMap, BlockId, Slot};
use std::collections::{btree_map, BTreeMap};
use tokio::sync::mpsc;
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
    _cfg: ExecutionSettings,
    /// VM
    vm: Arc<Mutex<VM>>,
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
    pending_css_final_blocks: BTreeMap<Slot, (BlockId, Block)>,
    /// VM thread
    vm_thread: JoinHandle<()>,
    /// VM execution requests queue
    execution_queue: ExecutionQueue,
}

impl ExecutionWorker {
    pub fn new(
        cfg: ExecutionSettings,
        thread_count: u8,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
        bootstrap_state: Option<BootstrapExecutionState>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        let execution_queue = ExecutionQueue::default();
        let execution_queue_clone = execution_queue.clone();
        let cfg_clone = cfg.clone();

        // Check bootstrap
        let bootstrap_final_slot;
        let bootstrap_ledger;
        if let Some(bootstrap_state) = bootstrap_state {
            // init from bootstrap
            bootstrap_final_slot = bootstrap_state.final_slot;
            bootstrap_ledger = Some((bootstrap_state.final_ledger, bootstrap_final_slot));
        } else {
            // init without bootstrap
            bootstrap_final_slot = Slot::new(0, thread_count.saturating_sub(1));
            bootstrap_ledger = None;
        };

        // Init VM
        let vm = Arc::new(Mutex::new(VM::new(cfg, thread_count, bootstrap_ledger)?));
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
            _cfg: cfg_clone,
            vm,
            thread_count,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
            //TODO bootstrap or init
            last_final_slot: bootstrap_final_slot,
            last_active_slot: bootstrap_final_slot,
            pending_css_final_blocks: Default::default(),
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

    pub async fn run_loop(mut self) -> Result<(), ExecutionError> {
        loop {
            tokio::select! {
                // Process management commands
                _ = self.controller_manager_rx.recv() => break,

                // Process commands
                Some(cmd) = self.controller_command_rx.recv() => self.process_command(cmd)?,
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

    fn blockclique_changed(
        &mut self,
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        // stop the current VM execution and reset state to final
        // TODO make something more iterative/conservative in the future to reuse unaffected executions
        self.reset_to_final();
        self.last_active_slot = self.last_final_slot;

        // process SCE-final slots
        self.pending_css_final_blocks
            .extend(finalized_blocks.into_iter().filter_map(|(b_id, b)| {
                if b.header.content.slot <= self.last_active_slot {
                    // eliminate blocks that are not from a stricly later slot than the current latest SCE-final one
                    return None;
                }
                Some((b.header.content.slot, (b_if, b)))
            }));
        if let Some(max_css_final_slot) = self
            .pending_css_final_blocks
            .last_key_value()
            .map(|(s, _v)| *s)
        {
            let mut cur_slot = self.last_final_slot.get_next_slot(self.thread_count)?;
            while cur_slot <= max_css_final_slot {
                match self
                    .pending_css_final_blocks
                    .first_key_value()
                    .map(|(s, _v)| *s)
                {
                    // there is a CSS-final block at cur_slot
                    Some(b_slot) if b_slot == cur_slot => {
                        // remove the entry from pending_css_final_blocks (cannot panic, checked above)
                        let Some((_b_slot, (b_id, block))) =
                            self.pending_css_final_blocks.pop_first().unwrap();
                        // execute the block as a SCE-final VM step
                        self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                            slot: cur_slot,
                            block: Some((b_id, block)),
                        }));
                        // update cursors
                        self.last_active_slot = cur_slot;
                        self.last_final_slot = cur_slot;
                    }

                    // there is no CSS-final block at cur_slot, but there are CSS-final blocks later
                    Some(_b_slot) => {
                        // check whether there is a CSS-final block later in the same thread
                        let mut check_slot = Slot::new(cur_slot.period + 1, cur_slot.thread);
                        while check_slot <= max_css_final_slot {
                            if self.pending_css_final_blocks.contains_key(&check_slot) {
                                break;
                            }
                            check_slot.period += 1;
                        }
                        if check_slot <= max_css_final_slot {
                            // subsequent CSS-final block found in the same thread as cur_slot
                            // execute a miss as an SCE-final VM step
                            self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                                slot: cur_slot,
                                block: None,
                            }));
                            // update cursors
                            self.last_active_slot = cur_slot;
                            self.last_final_slot = cur_slot;
                        } else {
                            // no subsequent CSS-final block found in the same thread as cur_slot
                            break;
                        }
                    }

                    // there are no more CSS-final blocks
                    None => break,
                }

                cur_slot = cur_slot.get_next_slot(self.thread_count)?;
            }
        }

        // process SCE-active blocks
        let sce_active_blocks: BTreeMap<Slot, (&BlockId, &Block)> = blockclique
            .iter()
            .filter_map(|(b_id, b)| {
                if b.header.content.slot <= self.last_final_slot {
                    // eliminate blocks that are not from a stricly later slot than the current latest SCE-final one
                    return None;
                }
                Some((b.header.content.slot, (b_id, b)))
            })
            .chain(self.pending_css_final_blocks.iter().map(|k, v| (*k, v)))
            .collect();
        if let Some(max_css_active_slot) = self.sce_active_blocks.last_key_value().map(|(s, _v)| *s)
        {
            let mut cur_slot = self.last_final_slot.get_next_slot(self.thread_count)?;
            while cur_slot <= max_css_active_slot {
                match sce_active_blocks.first_key_value().map(|(s, _v)| *s) {
                    // there is a CSS-active block at cur_slot
                    Some(b_slot) if b_slot == cur_slot => {
                        // remove the entry from sce_active_blocks (cannot panic, checked above)
                        let Some((_b_slot, (b_id, block))) = sce_active_blocks.pop_first().unwrap();
                        // execute the block as a SCE-active VM step
                        self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                            slot: cur_slot,
                            block: Some((*b_id, block.clone())),
                        }));
                        // update cursor
                        self.last_active_slot = cur_slot;
                    }

                    // there is no CSS-active block at cur_slot
                    Some(_b_slot) => {
                        // execute a miss as an SCE-active VM step
                        self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                            slot: cur_slot,
                            block: None,
                        }));
                        // update cursor
                        self.last_active_slot = cur_slot;
                    }

                    // there are no more CSS-active blocks
                    None => break,
                }

                cur_slot = cur_slot.get_next_slot(self.thread_count)?;
            }
        }

        // fill with misses until current slot
        //TODO

        Ok(())
    }
}
