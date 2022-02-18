use crate::controller::{ExecutionControllerImpl, ExecutionManagerImpl, VMInputData};
use crate::execution::ExecutionState;
use massa_execution_exports::{
    ExecutionConfig, ExecutionError, ExecutionOutput, ReadOnlyExecutionRequest,
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
/// structure gathering all elements needed by the VM thread
pub(crate) struct VMThread {
    // VM config
    config: ExecutionConfig,

    // VM data exchange controller
    controller: ExecutionControllerImpl,
    // map of SCE-final blocks not executed yet
    sce_finals: HashMap<Slot, Option<(BlockId, Block)>>,
    // last SCE final slot in sce_finals list
    last_sce_final: Slot,
    // map of CSS-final but non-SCE-final blocks
    remaining_css_finals: HashMap<Slot, (BlockId, Block)>,
    // last blockclique
    blockclique: HashMap<Slot, (BlockId, Block)>,
    // map of active slots
    active_slots: HashMap<Slot, Option<(BlockId, Block)>>,
    // highest active slot
    last_active_slot: Slot,

    // execution state
    execution_state: Arc<RwLock<ExecutionState>>,
}

impl VMThread {
    pub fn new(
        config: ExecutionConfig,
        controller: ExecutionControllerImpl,
        execution_state: Arc<RwLock<ExecutionState>>,
    ) -> Self {
        let final_cursor = execution_state
            .read()
            .expect("could not r-lock execution context")
            .final_cursor;

        // return VMThread
        VMThread {
            last_active_slot: final_cursor,
            controller,
            last_sce_final: final_cursor,
            sce_finals: Default::default(),
            remaining_css_finals: Default::default(),
            blockclique: Default::default(),
            active_slots: Default::default(),
            config,
            execution_state,
        }
    }

    /// update final slots
    fn update_final_slots(&mut self, new_css_finals: HashMap<Slot, (BlockId, Block)>) {
        // return if empty
        if new_css_finals.is_empty() {
            return;
        }

        // add new_css_finals to pending css finals
        self.remaining_css_finals.extend(new_css_finals);

        // get maximal css-final slot
        let max_css_final_slot = self
            .remaining_css_finals
            .iter()
            .max_by_key(|(s, _)| *s)
            .map(|(s, _)| *s)
            .expect("expected remaining_css_finals to be non-empty");

        // detect SCE-final slots
        let mut slot = self.last_sce_final;
        while slot < max_css_final_slot {
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("final slot overflow in VM");

            // pop slot from remaining CSS finals
            if let Some((block_id, block)) = self.remaining_css_finals.remove(&slot) {
                // CSS-final block found at slot: add block to to sce_finals
                self.sce_finals.insert(slot, Some((block_id, block)));
                self.last_sce_final = slot;
                // continue the loop
                continue;
            }

            // no CSS-final block found: it's a miss

            // check if the miss is final
            let mut miss_final = false;
            let mut search_slot = slot;
            while search_slot < max_css_final_slot {
                search_slot = search_slot
                    .get_next_slot(self.config.thread_count)
                    .expect("final slot overflow in VM");
                if self.remaining_css_finals.contains_key(&search_slot) {
                    miss_final = true;
                    break;
                }
            }

            if miss_final {
                // if the miss is final, set slot to be a final miss
                self.sce_finals.insert(slot, None);
                self.last_sce_final = slot;
            } else {
                // otherwise, this slot is not final => break
                break;
            }
        }
    }

    /// returns the end active slot (if any yet)
    /// this is the slot at which the cursor ends and it depends on the cursor_delay setting
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

    /// update active slot sequence
    fn update_active_slots(&mut self, new_blockclique: Option<HashMap<Slot, (BlockId, Block)>>) {
        // update blockclique if changed
        if let Some(blockclique) = new_blockclique {
            self.blockclique = blockclique;
        }

        // get end active slot, if any
        let end_active_slot = self.get_end_active_slot();

        // reset active slots
        self.active_slots = HashMap::new();
        self.last_active_slot = self.last_sce_final;

        // if no active slot yet => keep the active_slots empty
        let end_active_slot = match end_active_slot {
            Some(s) => s,
            None => return,
        };

        // recompute non-SCE-final slot sequence
        let mut slot = self.last_sce_final;
        while slot < end_active_slot {
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("active slot overflow in VM");
            if let Some((block_id, block)) = self.remaining_css_finals.get(&slot) {
                // found in remaining_css_finals
                self.active_slots
                    .insert(slot, Some((*block_id, block.clone())));
            } else if let Some((block_id, block)) = self.blockclique.get(&slot) {
                // found in blockclique
                self.active_slots
                    .insert(slot, Some((*block_id, block.clone())));
            } else {
                // miss
                self.active_slots.insert(slot, None);
            }
            self.last_active_slot = slot;
        }
    }

    /// executes one final slot, if any
    /// returns true if something was executed
    fn execute_one_final_slot(&mut self) -> bool {
        // check if there are final slots to execute
        if self.sce_finals.is_empty() {
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
            .sce_finals
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

        return true;
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

        return true;
    }

    /// gets the time until the next active slot (saturates down to 0)
    fn get_time_until_next_active_slot(&self) -> MassaTime {
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
        let now = MassaTime::compensated_now(self.config.clock_compensation)
            .expect("could not get current time in VM");
        next_timestmap.saturating_sub(now)
    }

    /// truncates history if necessary
    pub fn truncate_history(&mut self) {
        // acquire write access to execution state
        let mut exec_state = self
            .execution_state
            .write()
            .expect("could not lock execution state for writing");

        exec_state.truncate_history(&self.active_slots);
    }

    /// execute readonly request
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
        resp_tx: mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
    ) {
        // acquire read access to execution state and execute the request
        let outcome = self
            .execution_state
            .read()
            .expect("could not lock execution state for reading")
            .execute_readonly_request(req);

        // send the response
        if resp_tx.send(outcome).is_err() {
            debug!("could not send execute_readonly_request response: response channel died");
        }
    }

    /// main VM loop
    pub fn main_loop(&mut self) {
        loop {
            // read input queues
            let input_data = self.controller.consume_input();

            // check for stop signal
            if input_data.stop {
                break;
            }

            // update execution sequences
            if input_data.blockclique_changed {
                // changes detected in input

                // update final slot sequence
                self.update_final_slots(input_data.finalized_blocks);

                // update active slot sequence
                self.update_active_slots(Some(input_data.blockclique));
            }

            // execute one final slot, if any
            if self.execute_one_final_slot() {
                // a final slot was executed: continue
                continue;
            }

            // now all final slots have been executed

            // if the blockclique was not updated, still fill up active slots with misses until now()
            if !input_data.blockclique_changed {
                self.update_active_slots(None);
            }

            // truncate the speculative execution outputs if necessary
            if input_data.blockclique_changed {
                self.truncate_history();
            }

            // speculatively execute one active slot, if any
            if self.execute_one_active_slot() {
                // an active slot was executed: continue
                continue;
            }

            // execute all queued readonly requests
            // must be done in this loop because of the static shared context
            for (req, resp_tx) in input_data.readonly_requests {
                self.execute_readonly_request(req, resp_tx);
            }

            // check if new data or requests arrived during the iteration
            let input_data = self
                .controller
                .input_data
                .1
                .lock()
                .expect("could not lock VM input data");
            if input_data.stop {
                break;
            }
            if input_data.blockclique_changed || !input_data.readonly_requests.is_empty() {
                continue;
            }

            // compute when the next slot is
            let delay_until_next_slot = self.get_time_until_next_active_slot();
            if delay_until_next_slot == 0.into() {
                // next slot is right now
                continue;
            }

            // wait for change or for next slot
            let _ = self
                .controller
                .input_data
                .0
                .wait_timeout(input_data, delay_until_next_slot.to_duration())
                .expect("VM main loop condition variable wait failed");
        }

        // signal cancellation to all remaining readonly requests
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

/// launches the VM and returns a VMManager
///
/// # parameters
/// * config: VM configuration
/// * bootstrap:
pub fn start_vm(
    config: ExecutionConfig,
    final_ledger: Arc<RwLock<FinalLedger>>,
) -> ExecutionManagerImpl {
    // create an execution state
    let execution_state = Arc::new(RwLock::new(ExecutionState::new(
        config.clone(),
        final_ledger.clone(),
    )));

    // create a controller
    let controller = ExecutionControllerImpl {
        config: config.clone(),
        input_data: Arc::new((
            Condvar::new(),
            Mutex::new(VMInputData {
                blockclique_changed: true,
                ..Default::default()
            }),
        )),
        execution_state: execution_state.clone(),
    };

    // launch the VM thread
    let ctl = controller.clone();
    let thread_handle = std::thread::spawn(move || {
        VMThread::new(config, ctl, execution_state).main_loop();
    });

    // return the VM manager
    ExecutionManagerImpl {
        controller,
        thread_handle,
    }
}
