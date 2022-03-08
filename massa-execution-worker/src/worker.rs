// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module allows launching the execution worker thread, returning objects to communicate with it.
//! The worker thread processes incoming notifications of blockclique changes,
//! orders active and final blocks in queues sorted by increasing slot number,
//! and requests the execution of active and final slots from execution.rs.

use crate::controller::{ExecutionControllerImpl, ExecutionInputData, ExecutionManagerImpl};
use crate::execution::ExecutionState;
use crate::request_queue::RequestQueue;
use massa_execution_exports::{
    ExecutionConfig, ExecutionController, ExecutionError, ExecutionManager, ExecutionOutput,
    ReadOnlyExecutionRequest,
};
use massa_ledger::FinalLedger;
use massa_models::BlockId;
use massa_models::{
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
    Block, Slot,
};
use massa_time::MassaTime;
use parking_lot::{Condvar, Mutex, RwLock};
use std::{collections::HashMap, sync::Arc};

/// Structure gathering all elements needed by the execution thread
pub(crate) struct ExecutionThread {
    // Execution config
    config: ExecutionConfig,
    // A copy of the input data allowing access to incoming requests
    input_data: Arc<(Condvar, Mutex<ExecutionInputData>)>,
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
    /// queue for readonly execution requests and response mpscs to send back their outputs
    readonly_requests: RequestQueue<ReadOnlyExecutionRequest, ExecutionOutput>,
}

impl ExecutionThread {
    /// Creates the ExecutionThread structure to gather all data and references
    /// needed by the execution worker thread.
    ///
    /// # Arguments
    /// * config: execution config
    /// * input_data: a copy of the input data interface to get incoming requests from
    /// * execution_state: an thread-safe shared access to the execution state, which can be bootstrapped or newly created
    pub fn new(
        config: ExecutionConfig,
        input_data: Arc<(Condvar, Mutex<ExecutionInputData>)>,
        execution_state: Arc<RwLock<ExecutionState>>,
    ) -> Self {
        // get the latest executed final slot, at the output of which the final ledger is attached
        let final_cursor = execution_state.read().final_cursor;

        // create and return the ExecutionThread
        ExecutionThread {
            last_active_slot: final_cursor,
            input_data,
            last_ready_final_slot: final_cursor,
            ready_final_slots: Default::default(),
            pending_final_blocks: Default::default(),
            blockclique: Default::default(),
            active_slots: Default::default(),
            readonly_requests: RequestQueue::new(config.readonly_queue_length),
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
        let mut exec_state = self.execution_state.write();

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

    /// Check if there are any active slots ready for execution
    /// This is used to check if the main loop should run an iteration
    fn are_there_active_slots_ready_for_execution(&self) -> bool {
        let execution_state = self.execution_state.read();

        // get the next active slot
        let slot = execution_state
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("active slot overflow in VM");

        // check if it is in the active slot queue
        self.active_slots.contains_key(&slot)
    }

    /// executes one active slot, if any
    /// returns true if something was executed
    fn execute_one_active_slot(&mut self) -> bool {
        // write-lock the execution state
        let mut exec_state = self.execution_state.write();

        // get the next active slot
        let slot = exec_state
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("active slot overflow in VM");

        // choose the execution target
        let exec_target = match self.active_slots.get(&slot) {
            Some(b) => b.clone(), //TODO get rid of that clone on storage refactorig https://github.com/massalabs/massa/issues/2178
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
        let next_timestamp = get_block_slot_timestamp(
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
        next_timestamp.saturating_sub(end_time)
    }

    /// Tells the execution state about the new sequence of active slots.
    /// If some slots already executed in a speculative way changed,
    /// or if one of their have predecessor slots changed,
    /// the execution state will truncate the execution output history
    /// to remove all out-of-date execution outputs.
    /// Speculative execution will then resume from the point of truncation.
    pub fn truncate_execution_history(&mut self) {
        // acquire write access to execution state
        let mut exec_state = self.execution_state.write();

        // tells the execution state to truncate its execution output history
        // given the new list of active slots
        exec_state.truncate_history(&self.active_slots, &self.ready_final_slots);
    }

    /// Append incoming read-only requests to the relevant queue,
    /// Cancel those that are in excess if there are too many.
    fn update_readonly_requests(
        &mut self,
        new_requests: RequestQueue<ReadOnlyExecutionRequest, ExecutionOutput>,
    ) {
        // Append incoming readonly requests to our readonly request queue
        // Excess requests are cancelld
        self.readonly_requests.extend(new_requests);
    }

    /// Executes a read-only request from the queue, if any.
    /// The result of the execution is sent asynchronously through the response channel provided with the request.
    ///
    /// # Returns
    /// true if a request was executed, false otherwise
    fn execute_one_readonly_request(&mut self) -> bool {
        if let Some(req_resp) = self.readonly_requests.pop() {
            let (req, resp_tx) = req_resp.into_request_sender_pair();

            // Acquire read access to the execution state and execute the read-only request
            let outcome = self.execution_state.read().execute_readonly_request(req);

            // Send the execution output through resp_tx.
            // Ignore errors because they just mean that the request emitter dropped the received
            // because it doesn't need the response anymore.
            let _ = resp_tx.send(outcome);

            return true;
        }
        false
    }

    /// Waits for an event to trigger a new iteration in the excution main loop.
    ///
    /// # Returns
    /// Some(ExecutionInputData) representing the input requests,
    /// or None if the main loop needs to stop.
    fn wait_loop_event(&mut self) -> Option<ExecutionInputData> {
        let mut cancel_input = loop {
            let mut input_data_lock = self.input_data.1.lock();

            // take current input data, resetting it
            let input_data: ExecutionInputData = input_data_lock.take();

            // check for stop signal
            if input_data.stop {
                break input_data;
            }

            // Check for readonly requests, new blockclique or final slot changes
            // The most frequent triggers are checked first.
            if !input_data.readonly_requests.is_empty()
                || input_data.new_blockclique.is_some()
                || !input_data.finalized_blocks.is_empty()
            {
                return Some(input_data);
            }

            // Check for slots to execute.
            // The most frequent triggers are checked first,
            // except for the active slot check which is last because it is more expensive.
            if !self.readonly_requests.is_empty()
                || !self.ready_final_slots.is_empty()
                || self.are_there_active_slots_ready_for_execution()
            {
                return Some(input_data);
            }

            // No input data, and no slots to execute.

            // Compute when the next slot will be
            // This is useful to wait for the next speculative miss to append to active slots.
            let time_until_next_slot = self.get_time_until_next_active_slot();
            if time_until_next_slot == 0.into() {
                // next slot is right now: the loop needs to iterate
                return Some(input_data);
            }

            // Wait to be notified of new input, for at most time_until_next_slot
            // The return value is ignored because we don't care what woke up the condition variable.
            let _res = self
                .input_data
                .0
                .wait_for(&mut input_data_lock, time_until_next_slot.to_duration());
        };

        // The loop needs to quit

        // Cancel pending readonly requests
        let cancel_err = ExecutionError::ChannelError(
            "readonly execution cancelled because the execution worker is closing".into(),
        );
        cancel_input.readonly_requests.cancel(cancel_err.clone());
        self.input_data
            .1
            .lock()
            .take()
            .readonly_requests
            .cancel(cancel_err);

        None
    }

    /// Main loop of the executin worker
    pub fn main_loop(&mut self) {
        // This loop restarts everytime an execution happens for easier tracking.
        // It also prioritizes executions in the following order:
        // 1 - final executions
        // 2 - speculative executions
        // 3 - read-only executions
        while let Some(input_data) = self.wait_loop_event() {
            // update the sequence of final slots given the newly finalized blocks
            self.update_final_slots(input_data.finalized_blocks);

            // update the sequence of active slots
            self.update_active_slots(input_data.new_blockclique);

            // The list of active slots might have seen
            // new insertions/deletions of blocks at different slot depths.
            // It is therefore important to signal this to the execution state,
            // so that it can remove out-of-date speculative execution results from its history.
            self.truncate_execution_history();

            // update the sequence of read-only requests
            self.update_readonly_requests(input_data.readonly_requests);

            // execute one slot as final, if there is one ready for final execution
            if self.execute_one_final_slot() {
                // A slot was executed as final: restart the loop
                // This loop continue is useful for monitoring:
                // it allows tracking the state of all execution queues
                continue;
            }

            // now all the slots that were ready for final execution have been executed as final

            // Execute one active slot in a speculative way, if there is one ready for that
            if self.execute_one_active_slot() {
                // An active slot was executed: restart the loop
                continue;
            }

            // now all the slots that were ready for final and active execution have been executed

            // Execute a read-only request (note that the queue is of finite length), if there is one ready.
            // This must be done in this loop because even though read-only executions do not alter consensus state,
            // they still act temporarily on the static shared execution context.
            if self.execute_one_readonly_request() {
                // a read-only request was executed: restart the loop
                continue;
            }
        }
    }
}

/// Launches an execution worker thread and returns an ExecutionManager to interact with it
///
/// # parameters
/// * config: execution config
/// * final_ledger: a thread-safe shared access to the final ledger for reading and writing
///
/// # Returns
/// A pair (execution_manager, execution_controller) where:
/// * execution_manager allows to stop the worker
/// * execution_controller allows sending requests and notifications to the worker
pub fn start_execution_worker(
    config: ExecutionConfig,
    final_ledger: Arc<RwLock<FinalLedger>>,
) -> (Box<dyn ExecutionManager>, Box<dyn ExecutionController>) {
    // create an execution state
    let execution_state = Arc::new(RwLock::new(ExecutionState::new(
        config.clone(),
        final_ledger,
    )));

    // define the input data interface
    let input_data = Arc::new((
        Condvar::new(),
        Mutex::new(ExecutionInputData::new(config.clone())),
    ));

    // create a controller
    let controller = ExecutionControllerImpl {
        input_data: input_data.clone(),
        execution_state: execution_state.clone(),
    };

    // launch the execution thread
    let input_data_clone = input_data.clone();
    let thread_handle = std::thread::spawn(move || {
        ExecutionThread::new(config, input_data_clone, execution_state).main_loop();
    });

    // create a manager
    let manager = ExecutionManagerImpl {
        input_data,
        thread_handle: Some(thread_handle),
    };

    // return the execution manager and controller pair
    (Box::new(manager), Box::new(controller))
}
