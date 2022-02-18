//! # Execution worker component
//! --------------------------
//! ## Loops
//! The execution worker contains two threads, one in the main tokio runtime
//! in the massa-node and another parrallel detached from the tokio runtime.
//!
//! The execution of bytecodes lock a lot of CPU time so we execute it in a
//! pool worker in a dedicated thread `vm_thread`. This worker will execute
//! each actions contained in the queue in the FIFO queue `execution_queue`
//! (See: `run_vm_thread()`)
//!
//! ## Command passing
//! The commands send by the controller, are managed later in another OS
//! thread. The commands are dispached well by `process_command()` that will
//! among other things append `ExecutionRequest`s into the `execution_queue`.
//!
//! ## Retreive information from `vm`
//! The vm is contained by another thread and isn't shared between threads.
//! Retreiving information in theledger is done with requests as
//! `GetSCOutputEvents` in the `execution_queue`.
//!
//! ## Bytecode execution
//! The bytecode execution is done in `vm.rs`
//!
use crate::error::ExecutionError;
use crate::sce_ledger::{FinalLedger, SCELedger};
use crate::types::{ExecutionQueue, ExecutionRequest};
use crate::vm::VM;
use crate::BootstrapExecutionState;
use crate::{settings::ExecutionConfigs, types::ExecutionStep};
use massa_models::api::SCELedgerInfo;
use massa_models::execution::ExecuteReadOnlyResponse;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::Map;
use massa_models::timeslots::{get_block_slot_timestamp, get_current_latest_block_slot};
use massa_models::{Address, Amount, Block, BlockId, OperationId, Slot};
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
        blockclique: Map<BlockId, Block>,
        finalized_blocks: Map<BlockId, Block>,
    },

    /// Get a snapshot of the current state for bootstrap
    GetBootstrapState(tokio::sync::oneshot::Sender<BootstrapExecutionState>),

    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    GetSCOutputEvents {
        start: Option<Slot>,
        end: Option<Slot>,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
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
    GetSCELedgerForAddresses {
        response_tx: oneshot::Sender<Map<Address, SCELedgerInfo>>,
        addresses: Vec<Address>,
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
    /// Create a new execution worker.
    /// Start the worker and the vm threads. Init from bootstrap if
    /// `bootstrap_state` is Some, otherwise initialise from _Slot 0_
    pub fn new(
        cfg: ExecutionConfigs,
        event_sender: mpsc::UnboundedSender<ExecutionEvent>,
        controller_command_rx: mpsc::Receiver<ExecutionCommand>,
        controller_manager_rx: mpsc::Receiver<ExecutionManagementCommand>,
        bootstrap_state: Option<BootstrapExecutionState>,
    ) -> Result<ExecutionWorker, ExecutionError> {
        let execution_queue = ExecutionQueue::default();

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
        let vm_thread = run_vm_thread(cfg.clone(), bootstrap_ledger, execution_queue.clone())?;
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
    /// See: `process_command()`
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

    /// Start the main worker loop in the main tokio `Runtime`
    /// Dispatch the commands received by the controller with the
    /// `process_command()`
    ///
    /// # Error management
    /// The context of this function is in the main `Runtime` and the `Result`
    /// returned is managed by the main loop that will stop the node on error.
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

    /// Interact with the OS thread dispatching the given `cmd` through
    /// differents method.
    ///
    /// # Error management
    /// The `Result` here is managed in the main loop that will make the node
    /// stop.
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
            ExecutionCommand::GetSCOutputEvents {
                start,
                end,
                emitter_address,
                original_caller_address,
                original_operation_id,
                response_tx,
            } => self.push_request(ExecutionRequest::GetSCOutputEvents {
                start,
                end,
                emitter_address,
                original_caller_address,
                original_operation_id,
                response_tx,
            }),
            ExecutionCommand::GetSCELedgerForAddresses {
                response_tx,
                addresses,
            } => self.push_request(ExecutionRequest::GetSCELedgerForAddresses {
                response_tx,
                addresses,
            }),
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
        blockclique: Map<BlockId, Block>,
        finalized_blocks: Map<BlockId, Block>,
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
                        let (_b_slot, (_b_id, _block)) = sce_active_blocks
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

/// Start the vm thread that will loop and pop `ExecutionRequest` from the
/// `execution_queue`.
///
/// The ExecutionRequest are in a FIFO queue that are poped by another thread
/// and ask in a separated thread (to a `vm` object See `vm.rs`) to execute it.
///
/// * `RunFinalStep`
/// Add a new change in the `context.ledger_change` with the result from
/// new final items in the ledger. Prune the `final_events` in the vm object
/// that overflow the max value defined by `cfg.settings.max_final_events`.
/// See `vm.run_final_step()`
///
/// * `RunActiveStep`
/// Add in the vm a new `StepHistoryItem` without moving the ledger in the
/// context.
///
/// * `RunReadOnly`
/// Execute request without saving any ledger_changes
///
/// * `ResetToFinalState`
/// Clear the non-final steps history, required each time the blockclique
/// change. Each blockclick modification suppose that the vm `ledger_changes`
/// are obsoletes.
///
/// * `GetSCOutputEvents`
/// Write in the given mpsc the current events contained in the vm.
///
/// * `Shutdown`
/// Send to the vm to stop reading inputs, called only by the execution worker,
/// when his main loop is close (See `self.run_loop()`)
///
/// * `GetSCELedgerForAddresses`
/// Send a the SCELedgerInfo, get the sce ledger entry from the vm.
fn run_vm_thread(
    cfg: ExecutionConfigs,
    bootstrap_ledger: Option<(SCELedger, Slot)>,
    execution_queue: ExecutionQueue,
) -> Result<JoinHandle<()>, ExecutionError> {
    // Init VM
    let mut vm = VM::new(cfg.clone(), bootstrap_ledger)?;

    // Start VM thread
    Ok(thread::spawn(move || {
        let (lock, condvar) = &*execution_queue;
        let mut requests = lock.lock().unwrap();
        // Run until shutdown.
        loop {
            match requests.pop_front() {
                Some(ExecutionRequest::RunFinalStep(step)) => {
                    vm.run_final_step(step, cfg.settings.max_final_events);
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
                Some(ExecutionRequest::GetSCOutputEvents {
                    start,
                    end,
                    emitter_address,
                    original_caller_address,
                    original_operation_id,
                    response_tx,
                }) => {
                    if response_tx
                        .send(vm.get_filtered_sc_output_event(
                            start,
                            end,
                            emitter_address,
                            original_caller_address,
                            original_operation_id,
                        ))
                        .is_err()
                    {
                        debug!("execution: could not send get_sc_output_event_by_caller_address answer");
                    }
                }
                Some(ExecutionRequest::Shutdown) => return,
                Some(ExecutionRequest::GetSCELedgerForAddresses {
                    addresses,
                    response_tx,
                }) => {
                    let res = vm.get_sce_ledger_entry_for_addresses(addresses);
                    if response_tx.send(res).is_err() {
                        debug!("execution: could not send GetSCELedgerForAddresses response")
                    }
                }
                None => {
                    /* Condvar can self lock/unlock and the current line
                    robustify the behavior by ignoring it */
                    requests = condvar.wait(requests).unwrap();
                }
            };
        }
    }))
}
