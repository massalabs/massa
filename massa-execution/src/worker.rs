use crate::error::ExecutionError;
use crate::sce_ledger::FinalLedger;
use crate::types::{ExecutionQueue, ExecutionRequest};
use crate::vm::VM;
use crate::BootstrapExecutionState;
use crate::{config::ExecutionConfigs, types::ExecutionStep};
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::output_event::SCOutputEvent;
use massa_models::timeslots::{get_block_slot_timestamp, get_current_latest_block_slot};
use massa_models::{Address, Amount, Block, BlockHashMap, BlockId, Slot};
use std::collections::BTreeMap;
use std::thread::{self, JoinHandle};
use tokio::sync::{mpsc, oneshot};
use tokio::time::sleep_until;
use tracing::debug;

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

    GetSCOutputEventBySlotRange {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },

    GetSCOutputEventByCaller {
        caller_address: Address,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },

    GetSCOutputEventBySCAddress {
        sc_address: Address,
        response_tx: oneshot::Sender<Vec<SCOutputEvent>>,
    },

    /// Execute bytecode in read-only mode
    ExecuteReadOnlyRequest {
        /// Maximum gas spend in execution.
        max_gas: u64,
        /// The simulated price of gas for the read-only execution.
        simulated_gas_price: Amount,
        /// The code to execute.
        bytecode: Vec<u8>,
        /// The channel used to send the result of execution.
        result_sender: oneshot::Sender<ExecuteReadOnlyResponse>,
        /// The address, or a default random one if none is provided,
        /// which will simulate the sender of the operation.
        address: Option<Address>,
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
    cfg: ExecutionConfigs,
    /// Receiver of commands.
    controller_command_rx: mpsc::Receiver<ExecutionCommand>,
    /// Receiver of management commands.
    controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
    /// Sender of events.
    _event_sender: mpsc::UnboundedSender<ExecutionEvent>,
    /// Time cursors
    last_final_slot: Slot,
    last_active_slot: Slot,
    /// pending CSS final blocks
    pending_css_final_blocks: BTreeMap<Slot, (BlockId, Block)>,
    /// VM thread
    vm_thread: JoinHandle<()>,
    /// VM execution requests queue
    execution_queue: ExecutionQueue,
}

impl ExecutionWorker {
    pub fn new(
        cfg: ExecutionConfigs,
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
        let mut vm = VM::new(cfg.clone(), bootstrap_ledger)?;

        // Start VM thread
        let vm_thread = thread::spawn(move || {
            let (lock, condvar) = &*execution_queue_clone;
            let mut requests = lock.lock().unwrap();
            // Run until shutdown.
            loop {
                match requests.pop_front() {
                    Some(ExecutionRequest::RunFinalStep(step)) => {
                        vm.run_final_step(step);
                    }
                    Some(ExecutionRequest::RunActiveStep(step)) => {
                        vm.run_active_step(step);
                    }
                    Some(ExecutionRequest::RunReadOnly {
                        slot,
                        max_gas,
                        simulated_gas_price,
                        bytecode,
                        result_sender,
                        address,
                    }) => {
                        vm.run_read_only(
                            slot,
                            max_gas,
                            simulated_gas_price,
                            bytecode,
                            address,
                            result_sender,
                        );
                    }
                    Some(ExecutionRequest::ResetToFinalState) => vm.reset_to_final(),
                    Some(ExecutionRequest::GetBootstrapState { response_tx }) => {
                        let FinalLedger { ledger, slot } = vm.get_bootstrap_state();
                        let bootstrap_state = BootstrapExecutionState {
                            final_ledger: ledger,
                            final_slot: slot,
                        };
                        if response_tx.send(bootstrap_state).is_err() {
                            debug!("execution: could not send get_bootstrap_state answer");
                        }
                    }
                    Some(ExecutionRequest::Shutdown) => return,
                    None => {
                        requests = condvar.wait(requests).unwrap();
                    }
                };
            }
        });

        // return execution worker
        Ok(ExecutionWorker {
            cfg,
            controller_command_rx,
            controller_manager_rx,
            _event_sender: event_sender,
            last_final_slot: bootstrap_final_slot,
            last_active_slot: bootstrap_final_slot,
            pending_css_final_blocks: Default::default(),
            vm_thread,
            execution_queue,
        })
    }

    /// asks the VM to reset to its final state
    pub fn reset_to_final(&mut self) {
        let (queue_lock, condvar) = &*self.execution_queue;
        let queue_guard = &mut queue_lock.lock().unwrap();
        // cancel all non-final requests
        // Final execution requests are left to maintain final state consistency
        queue_guard.retain(|req| {
            matches!(
                req,
                ExecutionRequest::RunFinalStep(..)
                    | ExecutionRequest::Shutdown
                    | ExecutionRequest::GetBootstrapState { .. }
            )
        });
        // request reset to final state
        queue_guard.push_back(ExecutionRequest::ResetToFinalState);
        // notify
        condvar.notify_one();
    }

    /// sends an arbitrary VM request
    fn push_request(&self, request: ExecutionRequest) {
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
                self.push_request(ExecutionRequest::GetBootstrapState { response_tx });
            }

            ExecutionCommand::ExecuteReadOnlyRequest {
                max_gas,
                simulated_gas_price,
                bytecode,
                result_sender,
                address,
            } => {
                // call the VM to execute in read-only mode at the last active slot.
                self.push_request(ExecutionRequest::RunReadOnly {
                    slot: self.last_active_slot,
                    max_gas,
                    simulated_gas_price,
                    bytecode,
                    result_sender,
                    address,
                });
            }
            ExecutionCommand::GetSCOutputEventBySlotRange {
                start,
                end,
                response_tx,
            } => todo!(),
            ExecutionCommand::GetSCOutputEventByCaller {
                caller_address,
                response_tx,
            } => todo!(),
            ExecutionCommand::GetSCOutputEventBySCAddress {
                sc_address,
                response_tx,
            } => todo!(),
        }
        Ok(())
    }

    /// fills the remaining slots until now() with miss executions
    /// see step 4 in spec https://github.com/massalabs/massa/wiki/vm-block-feed
    fn fill_misses_until_now(&mut self) -> Result<(), ExecutionError> {
        /* TODO DISABLED TEMPORARILY https://github.com/massalabs/massa/issues/2101
        let end_step = get_current_latest_block_slot(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            self.cfg.clock_compensation,
        )?;
        if let Some(end_step) = end_step {
            // slot S
            let mut s = self.last_active_slot.get_next_slot(self.cfg.thread_count)?;

            while s <= end_step {
                // call the VM to execute an SCE-active miss at slot S
                self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                    slot: self.last_active_slot,
                    block: None,
                }));

                // set last_active_slot = S
                self.last_active_slot = s;

                s = s.get_next_slot(self.cfg.thread_count)?;
            }
        }
        */
        Ok(())
    }

    /// checks whether a miss at slot S would be SCE-final by looking up subsequent CSS-final blocks in the same thread
    /// see spec at https://github.com/massalabs/massa/wiki/vm-block-feed
    ///
    /// # Arguments
    /// * s: missed slot
    /// * max_css_final_slot: maximum lookup slot (included)
    fn is_miss_sce_final(&self, s: Slot, max_css_final_slot: Slot) -> bool {
        let mut check_slot = Slot::new(s.period + 1, s.thread);
        while check_slot <= max_css_final_slot {
            if self.pending_css_final_blocks.contains_key(&check_slot) {
                break;
            }
            check_slot.period += 1;
        }
        check_slot <= max_css_final_slot
    }

    /// called when the blockclique changes
    /// see spec at https://github.com/massalabs/massa/wiki/vm-block-feed
    fn blockclique_changed(
        &mut self,
        blockclique: BlockHashMap<Block>,
        finalized_blocks: BlockHashMap<Block>,
    ) -> Result<(), ExecutionError> {
        // 1 - reset the SCE state back to its latest final state

        // revert the VM to its latest SCE-final state by clearing its active slot history.
        // TODO make something more iterative/conservative in the future to reuse unaffected executions
        self.reset_to_final();

        self.last_active_slot = self.last_final_slot;

        // 2 - process CSS-final blocks

        // extend `pending_css_final_blocks` with `new_css_final_blocks`
        let new_css_final_blocks = finalized_blocks.into_iter().filter_map(|(b_id, b)| {
            if b.header.content.slot <= self.last_active_slot {
                // eliminate blocks that are not from a stricly later slot than the current latest SCE-final one
                // (this is an optimization)
                return None;
            }
            Some((b.header.content.slot, (b_id, b)))
        });
        self.pending_css_final_blocks.extend(new_css_final_blocks);

        if let Some(max_css_final_slot) = self
            .pending_css_final_blocks
            .last_key_value()
            .map(|(s, _v)| *s)
        {
            // iterate over every slot S starting from `last_final_slot.get_next_slot()` up to the latest slot in `pending_css_final_blocks` (included)
            let mut s = self.last_final_slot.get_next_slot(self.cfg.thread_count)?;
            while s <= max_css_final_slot {
                match self
                    .pending_css_final_blocks
                    .first_key_value()
                    .map(|(s, _v)| *s)
                {
                    // there is a block B at slot S in `pending_css_final_blocks`:
                    Some(b_slot) if b_slot == s => {
                        // remove B from `pending_css_final_blocks`
                        // cannot panic, checked above
                        let (_s, (b_id, b)) = self
                            .pending_css_final_blocks
                            .pop_first()
                            .expect("pending_css_final_blocks was unexpectedly empty");
                        // call the VM to execute the SCE-final block B at slot S
                        self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                            slot: s,
                            block: Some((b_id, b)),
                        }));

                        self.last_active_slot = s;
                        self.last_final_slot = s;
                    }

                    // there is no CSS-final block at s, but there are CSS-final blocks later
                    Some(_b_slot) => {
                        // check whether there is a CSS-final block later in the same thread
                        if self.is_miss_sce_final(s, max_css_final_slot) {
                            // subsequent CSS-final block found in the same thread as s
                            // call the VM to execute an SCE-final miss at slot S
                            self.push_request(ExecutionRequest::RunFinalStep(ExecutionStep {
                                slot: s,
                                block: None,
                            }));

                            self.last_active_slot = s;
                            self.last_final_slot = s;
                        } else {
                            // no subsequent CSS-final block found in the same thread as s
                            break;
                        }
                    }

                    // there are no more CSS-final blocks
                    None => break,
                }

                s = s.get_next_slot(self.cfg.thread_count)?;
            }
        }

        // 3 - process CSS-active blocks

        // define `sce_active_blocks = blockclique_blocks UNION pending_css_final_blocks`
        let new_blockclique_blocks = blockclique.iter().filter_map(|(b_id, b)| {
            if b.header.content.slot <= self.last_final_slot {
                // eliminate blocks that are not from a stricly later slot than the current latest SCE-final one
                // (this is an optimization)
                return None;
            }
            Some((b.header.content.slot, (b_id, b)))
        });
        let mut sce_active_blocks: BTreeMap<Slot, (&BlockId, &Block)> = new_blockclique_blocks
            .chain(
                self.pending_css_final_blocks
                    .iter()
                    .map(|(k, (b_id, b))| (*k, (b_id, b))),
            )
            .collect();

        if let Some(max_css_active_slot) = sce_active_blocks.last_key_value().map(|(s, _v)| *s) {
            // iterate over every slot S starting from `last_active_slot.get_next_slot()` up to the latest slot in `sce_active_blocks` (included)
            let mut s = self.last_final_slot.get_next_slot(self.cfg.thread_count)?;
            while s <= max_css_active_slot {
                let first_sce_active_slot = sce_active_blocks.first_key_value().map(|(s, _v)| *s);
                match first_sce_active_slot {
                    // there is a block B at slot S in `sce_active_blocks`:
                    Some(b_slot) if b_slot == s => {
                        // remove the entry from sce_active_blocks (cannot panic, checked above)
                        let (_b_slot, (b_id, block)) = sce_active_blocks
                            .pop_first()
                            .expect("sce_active_blocks should not be empty");
                        // call the VM to execute the SCE-active block B at slot S
                        /* TODO DISABLED TEMPORARILY https://github.com/massalabs/massa/issues/2101
                        self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                            slot: s,
                            block: Some((*b_id, block.clone())),
                        }));
                        self.last_active_slot = s;
                        */
                    }

                    // otherwise, if there is no CSS-active block at S
                    Some(b_slot) => {
                        // make sure b_slot is after s
                        if b_slot <= s {
                            panic!("remaining CSS-active blocks should be later than S");
                        }

                        // call the VM to execute an SCE-active miss at slot S
                        /*  TODO DISABLED TEMPORARILY https://github.com/massalabs/massa/issues/2101
                        self.push_request(ExecutionRequest::RunActiveStep(ExecutionStep {
                            slot: s,
                            block: None,
                        }));
                        self.last_active_slot = s;
                        */
                    }

                    // there are no more CSS-active blocks
                    None => break,
                }

                s = s.get_next_slot(self.cfg.thread_count)?;
            }
        }

        // 4 - fill the remaining slots with misses
        self.fill_misses_until_now()?;

        Ok(())
    }
}
