// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module allows launching the execution worker thread, returning objects to communicate with it.
//! The worker thread processes incoming notifications of blockclique changes,
//! orders active and final blocks in queues sorted by increasing slot number,
//! and requests the execution of active and final slots from execution.rs.

use crate::controller::{ExecutionControllerImpl, ExecutionManagerImpl, VMInputData};
use crate::execution::ExecutionState;
use massa_execution_exports::{
    ExecutionConfig, ExecutionError, ExecutionManager, ExecutionOutput, ReadOnlyExecutionRequest,
};
use massa_ledger::FinalLedger;
use massa_models::BlockId;
use massa_models::{
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
    Block, Slot,
};
use massa_time::MassaTime;
use std::sync::mpsc;
use std::{
    collections::HashMap,
    sync::{Arc, Condvar, Mutex, RwLock},
};
use tracing::debug;

/// Structure gathering all elements needed by the execution thread
pub(crate) struct ExecutionThread {
    // Execution config
    config: ExecutionConfig,
    // A copy of the controller allowing access to incoming requests
    controller: ExecutionControllerImpl,
    // Map of final slots not executed yet but ready for execution
    // See lib.rs for an explanation on final execution ordering.
    ready_final_slots: HashMap<Slot, Option<(BlockId, Block)>>,
    // Highest final slot that is ready to be executed
    last_ready_final_slot: Slot,
    // Map of final blocks that are not yet ready to be executed
    // See lib.rs for an explanation on final execution ordering.
    pending_final_blocks: HashMap<Slot, (BlockId, Block)>,
    // Current blockclique, indexed by slot number
    blockclique: HashMap<Slot, (BlockId, Block)>,
    // Map of all active slots
    active_slots: HashMap<Slot, Option<(BlockId, Block)>>,
    // Highest active slot
    last_active_slot: Slot,
    // Execution state (see execution.rs) to which execution requests are sent
    execution_state: Arc<RwLock<ExecutionState>>,
}

impl ExecutionThread {
    /// Creates the ExecutionThread structure to gather all data and references
    /// needed by the execution worker thread.
    ///
    /// # Arguments
    /// * config: execution config
    /// * controller: a copy of the ExecutionController to get incoming requests from
    /// * execution_state: an exclusive reference to the execution state, which can be bootstrapped or newly created
    pub fn new(
        config: ExecutionConfig,
        controller: ExecutionControllerImpl,
        execution_state: Arc<RwLock<ExecutionState>>,
    ) -> Self {
        // get the latest executed final slot, at the output of which the final ledger is attached
        let final_cursor = execution_state
            .read()
            .expect("could not r-lock execution context")
            .final_cursor;

        // create and return the ExecutionThread
        ExecutionThread {
            last_active_slot: final_cursor,
            controller,
            last_ready_final_slot: final_cursor,
            ready_final_slots: Default::default(),
            pending_final_blocks: Default::default(),
            blockclique: Default::default(),
            active_slots: Default::default(),
            config,
            execution_state,
        }
    }

    /// Update the sequence of final slots given newly finalized blocks.
    /// This method is called from the execution worker's main loop.
    ///
    /// # Arguments
    /// * new_final_blocks: a map of newly finalized blocks
    fn update_final_slots(&mut self, new_final_blocks: HashMap<Slot, (BlockId, Block)>) {
        // if there are no new final blocks, exit and do nothing
        if new_final_blocks.is_empty() {
            return;
        }

        // add new_final_blocks to the pending final blocks not ready for execution yet
        self.pending_final_blocks.extend(new_final_blocks);

        // get maximal final slot
        let max_final_slot = self
            .pending_final_blocks
            .iter()
            .max_by_key(|(s, _)| *s)
            .map(|(s, _)| *s)
            .expect("expected pending_final_blocks to be non-empty");

        // Given pending_final_blocks, detect he final slots that are ready to be executed.
        // Those are the ones or which all the previous slots are also executed or ready to be so.
        // Iterate over consecutive slots starting from the one just after the previous last final one.
        let mut slot = self.last_ready_final_slot;
        while slot < max_final_slot {
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("final slot overflow in VM");

            // try to remove that slot out of pending_final_blocks
            if let Some((block_id, block)) = self.pending_final_blocks.remove(&slot) {
                // pending final block found at slot:
                // add block to the ready_final_slots list of final slots ready for execution
                self.ready_final_slots.insert(slot, Some((block_id, block)));
                self.last_ready_final_slot = slot;
                // continue the loop
                continue;
            }

            // no final block found at this slot: it's a miss

            // check if the miss is final by searching for final blocks later in the same thread
            let mut miss_final = false;
            let mut search_slot = slot;
            while search_slot < max_final_slot {
                search_slot = search_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("final slot overflow in VM");
                if self.pending_final_blocks.contains_key(&search_slot) {
                    // A final block was found later in the same thread.
                    // The missed slot is therefore final.
                    miss_final = true;
                    break;
                }
            }

            if miss_final {
                // If this slot is a final miss
                // Add it to the list of final slots ready for execution
                self.ready_final_slots.insert(slot, None);
                self.last_ready_final_slot = slot;
            } else {
                // This slot is not final:
                // we have reached the end of the list of consecutive final slots
                // that are ready to be executed
                break;
            }
        }
    }

    /// Returns the latest slot that is at or just before the current timestamp.
    /// If a non-zero cursor_delay config is defined, this extra lag is taken into account.
    /// Such an extra lag can be useful for weaker nodes to perform less speculative executions
    /// because more recent slots change more often and might require multiple re-executions.
    ///
    /// # Returns
    /// The latest slot at or before now() - self.config.cursor_delay) if there is any,
    /// or None if it falls behind the genesis timestamp.
    fn get_end_active_slot(&self) -> Option<Slot> {
        let target_time = MassaTime::compensated_now(self.config.clock_compensation)
            .expect("could not read current time")
            .saturating_sub(self.config.cursor_delay);
        get_latest_block_slot_at_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            target_time,
        )
        .expect("could not get current slot")
    }

    /// Updates the sequence of active slots.
    /// If a new blockclique is provided, it is taken into account.
    /// If no blockclique is provided, this function is still useful to fill
    /// ready-to-be-executed active slots with misses until the current time.
    ///
    /// Arguments:
    /// * new_blockclique: optionally provide a new blockclique
    fn update_active_slots(&mut self, new_blockclique: Option<HashMap<Slot, (BlockId, Block)>>) {
        // Update the current blockclique if it has changed
        if let Some(blockclique) = new_blockclique {
            self.blockclique = blockclique;
        }

        // Get the latest slot at the current timestamp, if any
        let end_active_slot = self.get_end_active_slot();

        // Empty the list of active slots
        self.active_slots = HashMap::new();
        self.last_active_slot = self.last_ready_final_slot;

        // If the current timestamp is before genesis time, keep the active_slots empty and return
        let end_active_slot = match end_active_slot {
            Some(s) => s,
            None => return,
        };

        // Recompute the sequence of active slots
        // by iterating over consecutive slots from the one just after last_ready_final_slot until the current timestamp,
        // and looking for blocks into pending_final_blocks and the current blockclique
        let mut slot = self.last_ready_final_slot;
        while slot < end_active_slot {
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("active slot overflow in VM");
            // look for a block at that slot among the ones that are final but not ready for final execution yet
            if let Some((block_id, block)) = self.pending_final_blocks.get(&slot) {
                // A block at that slot was found in pending_final_blocks.
                // Add it to the sequence of active slots.
                self.active_slots
                    .insert(slot, Some((*block_id, block.clone())));
                self.last_active_slot = slot;
            } else if let Some((block_id, block)) = self.blockclique.get(&slot) {
                // A block at that slot was found in the current blockclique.
                // Add it to the sequence of active slots.
                self.active_slots
                    .insert(slot, Some((*block_id, block.clone())));
                self.last_active_slot = slot;
            } else {
                // No block was found at that slot: it's a miss
                // Add the miss to the sequence of active slots
                self.active_slots.insert(slot, None);
                self.last_active_slot = slot;
            }
        }
    }

    /// executes one final slot, if any
    /// returns true if something was executed
    fn execute_one_final_slot(&mut self) -> bool {
        // check if there are final slots to execute
        if self.ready_final_slots.is_empty() {
            return false;
        }

        // w-lock execution state
        let mut exec_state = self
            .execution_state
            .write()
            .expect("could not lock execution state for writing");

        // get the slot just after the last executed final slot
        let slot = exec_state
            .final_cursor
            .get_next_slot(self.config.thread_count)
            .expect("final slot overflow in VM");

        // take the corresponding element from sce finals
        let exec_target = self
            .ready_final_slots
            .remove(&slot)
            .expect("the SCE final slot list skipped a slot");

        // check if the final slot is cached at the front of the speculative execution history
        if let Some(exec_out) = exec_state.active_history.pop_front() {
            if exec_out.slot == slot
                && exec_out.block_id == exec_target.as_ref().map(|(b_id, _)| *b_id)
            {
                // speculative execution front result matches what we want to compute

                // apply the cached output and return
                exec_state.apply_final_execution_output(exec_out);
                return true;
            }
        }

        // speculative cache mismatch

        // clear the speculative execution output cache completely
        exec_state.clear_history();

        // execute slot
        let exec_out = exec_state.execute_slot(slot, exec_target);

        // apply execution output to final state
        exec_state.apply_final_execution_output(exec_out);

        true
    }

    /// executes one active slot, if any
    /// returns true if something was executed
    fn execute_one_active_slot(&mut self) -> bool {
        // write-lock the execution state
        let mut exec_state = self
            .execution_state
            .write()
            .expect("could not lock execution state for writing");

        // get the next active slot
        let slot = exec_state
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("active slot overflow in VM");

        // choose the execution target
        let exec_target = match self.active_slots.get(&slot) {
            Some(b) => b.clone(), //TODO get rid of that clone
            None => return false,
        };

        // execute the slot
        let exec_out = exec_state.execute_slot(slot, exec_target);

        // apply execution output to active state
        exec_state.apply_active_execution_output(exec_out);

        true
    }

    /// Gets the time from now() to the slot just after next last_active_slot.
    /// Saturates down to 0 on negative durations.
    /// Note that config.cursor_delay is taken into account.
    fn get_time_until_next_active_slot(&self) -> MassaTime {
        // get the timestamp of the slot after the current last active one
        let next_slot = self
            .last_active_slot
            .get_next_slot(self.config.thread_count)
            .expect("active slot overflow in VM");
        let next_timestmap = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            next_slot,
        )
        .expect("could not compute block timestmap in VM");

        // get the current timestamp minus the cursor delay
        let end_time = MassaTime::compensated_now(self.config.clock_compensation)
            .expect("could not get current time in VM")
            .saturating_sub(self.config.cursor_delay);

        // compute the time difference, saturating down to zero
        next_timestmap.saturating_sub(end_time)
    }

    /// Tells the execution state about the new sequence of active slots.
    /// If some slots already executed in a speculative way changed,
    /// or if one of their have predecessor slots changed,
    /// the execution state will truncate the execution output history
    /// to remove all out-of-date execution outputs.
    /// Speculative execution will then resume from the point of truncation.
    pub fn truncate_execution_history(&mut self) {
        // acquire write access to execution state
        let mut exec_state = self
            .execution_state
            .write()
            .expect("could not lock execution state for writing");

        // tells the execution state to truncate its execution output history
        // given the new list of active slots
        exec_state.truncate_history(&self.active_slots);
    }

    /// Executes a read-only request, and asynchronously returns the result once finished.
    ///
    /// # Arguments
    /// * req: read-only execution request parameters
    /// * resp_tx: MPSC sender through which the execution output is sent when the execution is over
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
        resp_tx: mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
    ) {
        // acquire read access to execution state and execute the read-only request
        let outcome = self
            .execution_state
            .read()
            .expect("could not lock execution state for reading")
            .execute_readonly_request(req);

        // send the execution output through resp_tx
        if resp_tx.send(outcome).is_err() {
            debug!("could not send execute_readonly_request response: response channel died");
        }
    }

    /// Main loop of the executin worker
    pub fn main_loop(&mut self) {
        loop {
            // read input requests
            let input_data = self.controller.consume_input();

            // check for stop signal
            if input_data.stop {
                break;
            }

            // if the blockclique has changed
            if input_data.blockclique_changed {
                // update the sequence of final slots given the newly finalized blocks
                self.update_final_slots(input_data.finalized_blocks);

                // update the sequence of active slots given the new blockclique
                self.update_active_slots(Some(input_data.blockclique));
            }

            // execute one slot as final, if there is one ready for final execution
            if self.execute_one_final_slot() {
                // A slot was executed as final: restart the loop
                // This loop continue is useful for monitoring:
                // it allows tracking the state of all execution queues
                continue;
            }

            // now all the slots that were ready for final execution have been executed as final

            // if the blockclique was not updated, the update_active_slots hasn't been called previously.
            // But we still fill up active slots with misses until now() so we call it with None as argument.
            if !input_data.blockclique_changed {
                self.update_active_slots(None);
            }

            // If the blockclique has changed, the list of active slots might have seen
            // new insertions/deletions of blocks at different slot depths.
            // It is therefore important to signal this to the execution state,
            // so that it can remove out-of-date speculative execution results from its history.
            if input_data.blockclique_changed {
                self.truncate_execution_history();
            }

            // Execute one active slot in a speculative way, if there is one ready for that
            if self.execute_one_active_slot() {
                // An active slot was executed: restart the loop
                // This loop continue is useful for monitoring:
                // it allows tracking the state of all execution queues,
                // as well as prioritizing executions in the following order:
                // 1 - final executions
                // 2 - speculative executions
                // 3 - read-only executions
                continue;
            }

            // Execute all queued readonly requests (note that the queue is of finite length)
            // This must be done in this loop because even though read-only executions do not alter consensus state,
            // they still act temporarily on the static shared execution context.
            for (req, resp_tx) in input_data.readonly_requests {
                self.execute_readonly_request(req, resp_tx);
            }

            // Peek into the input data to see if new input arrived during this iteration of the loop
            let input_data = self
                .controller
                .input_data
                .1
                .lock()
                .expect("could not lock execution input data");
            if input_data.stop {
                // there is a request to stop: quit the loop
                break;
            }
            if input_data.blockclique_changed || !input_data.readonly_requests.is_empty() {
                // there are blockclique updates or read-only requests: restart the loop
                continue;
            }

            // Here, we know that there is currently nothing to do for this worker

            // Compute when the next slot will be
            // This is useful to wait for the next speculative miss to append to active slots.
            let time_until_next_slot = self.get_time_until_next_active_slot();
            if time_until_next_slot == 0.into() {
                // next slot is right now: simply restart the loop
                continue;
            }

            // Wait to be notified of new input, for at most time_until_next_slot
            // Note: spurious wake-ups are not a problem:
            // the next loop iteration will just do nohing and come back to wait here.
            let (_lock, _timeout_result) = self
                .controller
                .input_data
                .0
                .wait_timeout(input_data, time_until_next_slot.to_duration())
                .expect("Execution worker main loop condition variable wait failed");
        }

        // the execution worker is stopping:
        // signal cancellation to all remaining read-only execution requests waiting for an MPSC response
        let mut input_data = self
            .controller
            .input_data
            .1
            .lock()
            .expect("could not lock VM input data");
        for (_req, resp_tx) in input_data.readonly_requests.drain(..) {
            if resp_tx
                .send(Err(ExecutionError::RuntimeError(
                    "readonly execution cancelled because VM is closing".into(),
                )))
                .is_err()
            {
                debug!("failed sending readonly request response: channel down");
            }
        }
    }
}

/// Launches an execution worker thread and returns an ExecutionManager to interact with it
///
/// # parameters
/// * config: execution config
/// * final_ledger: a reference to the final ledger for shared reading and exclusive writing
///
/// # Returns
/// An instance of ExecutionManager allowing to stop the worker or generate ExecutionController instances,
/// which are used to send requests and notifications to the worker.
pub fn start_execution_worker(
    config: ExecutionConfig,
    final_ledger: Arc<RwLock<FinalLedger>>,
) -> Box<dyn ExecutionManager> {
    // create an execution state
    let execution_state = Arc::new(RwLock::new(ExecutionState::new(
        config.clone(),
        final_ledger,
    )));

    // create a controller
    let controller = ExecutionControllerImpl {
        config: config.clone(),
        input_data: Arc::new((
            Condvar::new(),
            Mutex::new(VMInputData {
                // ntify of a blockclique change to run one initialization loop itration
                blockclique_changed: true,
                ..Default::default()
            }),
        )),
        execution_state: execution_state.clone(),
    };

    // launch the execution thread
    let ctl = controller.clone();
    let thread_handle = std::thread::spawn(move || {
        ExecutionThread::new(config, ctl, execution_state).main_loop();
    });

    // return the execution manager
    Box::new(ExecutionManagerImpl {
        controller,
        thread_handle: Some(thread_handle),
    })
}
