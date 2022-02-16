use crate::speculative_ledger::SpeculativeLedger;
use crate::ExecutionError;
use massa_ledger::{Applicable, FinalLedger, LedgerChanges, LedgerEntry, LedgerEntryUpdate};
use massa_models::{
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
    Block, Slot,
};
use massa_models::{Address, Amount, BlockId};
use massa_time::MassaTime;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex, RwLock},
};
use tracing::info;

/// VM module configuration
pub struct VMConfig {
    /// number of threads
    thread_count: u8,
    /// extra lag to add on the cursor to improve performance
    cursor_delay: MassaTime,
    /// time compensation in milliseconds
    clock_compensation: i64,
    /// genesis timestamp
    genesis_timestamp: MassaTime,
    /// period duration
    t0: MassaTime,
}

/// structure describing a read-only execution request
pub struct ReadOnlyExecutionRequest {
    /// The slot at which the execution will occur.
    slot: Slot,
    /// Maximum gas to spend in the execution.
    max_gas: u64,
    /// The simulated price of gas for the read-only execution.
    simulated_gas_price: Amount,
    /// The code to execute.
    bytecode: Vec<u8>,
    /// Call stack to simulate
    call_stack: Vec<Address>,
    /// The channel used to send the result of the execution.
    result_sender: std::sync::mpsc::Sender<Result<ExecutionOutput, ExecutionError>>,
}

/// structure used to communicate with the VM thread
#[derive(Default)]
pub struct VMInputData {
    /// set stop to true to stop the thread
    stop: bool,
    /// signal whether the blockclique changed
    blockclique_changed: bool,
    /// list of newly finalized blocks
    finalized_blocks: HashMap<Slot, (BlockId, Block)>,
    /// blockclique
    blockclique: HashMap<Slot, (BlockId, Block)>,
    /// readonly execution requests
    readonly_requests: VecDeque<ReadOnlyExecutionRequest>,
}

/// VM controller
pub struct VMController {
    /// condition variable to wake up the VM loop
    loop_cv: Condvar,
    /// input data to process in the VM loop
    input_data: Mutex<VMInputData>,
}

/// VM manager
pub struct VMManager {
    /// shared reference to the VM controller
    controller: Arc<VMController>,
    /// handle used to join the VM thread
    thread_handle: std::thread::JoinHandle<()>,
}

impl VMManager {
    /// stops the VM
    pub fn stop(self) {
        info!("stopping VM controller...");
        // notify the VM thread to stop
        {
            let mut input_wlock = self
                .controller
                .input_data
                .lock()
                .expect("could not w-lock VM input data");
            input_wlock.stop = true;
            self.controller.loop_cv.notify_one();
        }
        // join the VM thread
        self.thread_handle
            .join()
            .expect("VM controller thread panicked");
        info!("VM controller stopped");
    }

    /// get a shared reference to the VM controller
    pub fn get_controller(&self) -> Arc<VMController> {
        self.controller.clone()
    }
}

/// launches the VM and returns a VMManager
///
/// # parameters
/// * config: VM configuration
/// * bootstrap:
pub fn start_vm(config: VMConfig, final_ledger: Arc<RwLock<FinalLedger>>) -> VMManager {
    let controller = Arc::new(VMController {
        loop_cv: Condvar::new(),
        input_data: Mutex::new(VMInputData {
            blockclique_changed: true,
            ..Default::default()
        }),
    });

    let ctl = controller.clone();
    let thread_handle = std::thread::spawn(move || {
        VMThread::new(config, ctl, final_ledger).main_loop();
    });

    VMManager {
        controller,
        thread_handle,
    }
}

struct ExecutionOutput {
    // slot
    slot: Slot,
    // optional block ID at that slot (None if miss)
    block_id: Option<BlockId>,
    // ledger_changes caused by the execution step
    ledger_changes: LedgerChanges,
    // events emitted by the execution step
    //TODO events: EventStore
}

/// structure gathering all elements needed by the VM thread
struct VMThread {
    // VM config
    config: VMConfig,
    // Final ledger
    final_ledger: Arc<RwLock<FinalLedger>>,
    // VM data exchange controller
    controller: Arc<VMController>,
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
    // final execution cursor
    final_cursor: Slot,
    // active execution cursor
    active_cursor: Slot,
    // execution output history
    execution_history: VecDeque<ExecutionOutput>,
    // execution context
    execution_context: Arc<Mutex<ExecutionContext>>,
    // final events
    // final_events: EventStore,
}

pub(crate) struct ExecutionContext {
    //TODO other things (eg. call stack)
    speculative_ledger: SpeculativeLedger,
    //TODO event store
}

impl VMThread {
    fn new(
        config: VMConfig,
        controller: Arc<VMController>,
        final_ledger: Arc<RwLock<FinalLedger>>,
    ) -> Self {
        let final_slot = final_ledger
            .read()
            .expect("could not R-lock final ledger in VM thread creation")
            .slot;
        let execution_context = Arc::new(Mutex::new(ExecutionContext {
            speculative_ledger: SpeculativeLedger::new(
                final_ledger.clone(),
                LedgerChanges::default(),
            ),
        }));

        VMThread {
            final_ledger,
            last_active_slot: final_slot,
            final_cursor: final_slot,
            active_cursor: final_slot,
            controller,
            last_sce_final: final_slot,
            execution_context,
            sce_finals: Default::default(),
            remaining_css_finals: Default::default(),
            blockclique: Default::default(),
            active_slots: Default::default(),
            execution_history: Default::default(),
            config,
        }
    }

    /// reads the list of newly finalized blocks and the new blockclique, if there was a change
    /// if found, remove from input queue
    fn consume_input(&mut self) -> VMInputData {
        std::mem::take(
            &mut self
                .controller
                .input_data
                .lock()
                .expect("VM input data lock failed"),
        )
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

    /// applies an execution output to the final state
    fn apply_final_execution_output(&mut self, exec_out: ExecutionOutput) {
        // update cursors
        self.final_cursor = exec_out.slot;
        if self.active_cursor <= self.final_cursor {
            self.final_cursor = self.final_cursor;
        }

        // apply final ledger changes
        {
            let mut final_ledger = self
                .final_ledger
                .write()
                .expect("could not lock final ledger for writing");
            final_ledger.settle_slot(exec_out.slot, exec_out.ledger_changes);
        }

        // save generated events to final store
        // TODO
    }

    /// applies an execution output to the active state
    fn apply_active_execution_output(&mut self, exec_out: ExecutionOutput) {
        // update active cursor
        self.active_cursor = exec_out.slot;

        // add execution output to history
        self.execution_history.push_back(exec_out);
    }

    /// returns the speculative ledger at a given history slot
    fn get_speculative_ledger_at_slot(&self, slot: Slot) -> SpeculativeLedger {
        // check that the slot is within the reach of history
        if slot <= self.final_cursor {
            panic!("cannot execute at a slot before finality");
        }
        let max_slot = self
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow when getting speculative ledger");
        if slot > max_slot {
            panic!("cannot execute at a slot beyond active cursor + 1");
        }

        // gather the history of changes
        let mut previous_ledger_changes = LedgerChanges::default();
        for previous_output in &self.execution_history {
            if previous_output.slot >= slot {
                break;
            }
            previous_ledger_changes.apply(&previous_output.ledger_changes);
        }

        // return speculative ledger
        SpeculativeLedger::new(self.final_ledger.clone(), previous_ledger_changes)
    }

    /// executes a full slot without causing any changes to the state,
    /// and yields an execution output
    fn execute_slot(&mut self, slot: Slot, opt_block: Option<(BlockId, Block)>) -> ExecutionOutput {
        // get the speculative ledger
        let ledger = self.get_speculative_ledger_at_slot(slot);

        // TODO init context

        // TODO intial executions

        // TODO async executions

        let mut out_block_id = None;
        if let Some((block_id, block)) = opt_block {
            out_block_id = Some(block_id);

            //TODO block stuff
        }

        ExecutionOutput {
            slot,
            block_id: out_block_id,
            ledger_changes: ledger.into_added_changes(),
        }
    }

    /// clear execution history
    fn clear_history(&mut self) {
        // clear history
        self.execution_history.clear();

        // reset active cursor
        self.active_cursor = self.final_cursor;
    }

    /// executes one final slot, if any
    /// returns true if something was executed
    fn execute_one_final_slot(&mut self) -> bool {
        // check if there are final slots to execute
        if self.sce_finals.is_empty() {
            return false;
        }

        // get the slot just after the last executed final slot
        let slot = self
            .final_cursor
            .get_next_slot(self.config.thread_count)
            .expect("final slot overflow in VM");

        // take element from sce finals
        let exec_target = self
            .sce_finals
            .remove(&slot)
            .expect("the SCE final slot list skipped a slot");

        // check if the final slot is cached at the front of the speculative execution history
        if let Some(exec_out) = self.execution_history.pop_front() {
            if exec_out.slot == slot
                && exec_out.block_id == exec_target.as_ref().map(|(b_id, _)| *b_id)
            {
                // speculative execution front result matches what we want to compute
                self.apply_final_execution_output(exec_out);
                return true;
            }
        }

        // speculative cache mismatch

        // clear the speculative execution output cache completely
        self.clear_history();

        // execute slot
        let exec_out = self.execute_slot(slot, exec_target);

        // apply execution output to final state
        self.apply_final_execution_output(exec_out);

        return true;
    }

    /// truncates active slots at the first mismatch
    /// between the active execution output history and the planned active_slots
    fn truncate_history(&mut self) {
        // find mismatch point (included)
        let mut truncate_at = None;
        for (hist_index, exec_output) in self.execution_history.iter().enumerate() {
            let found_block_id = self
                .active_slots
                .get(&exec_output.slot)
                .map(|opt_b| opt_b.as_ref().map(|(b_id, b)| *b_id));
            if found_block_id == Some(exec_output.block_id) {
                continue;
            }
            truncate_at = Some(hist_index);
            break;
        }

        // truncate speculative execution output history
        if let Some(truncate_at) = truncate_at {
            self.execution_history.truncate(truncate_at);
            self.active_cursor = self
                .execution_history
                .back()
                .map_or(self.final_cursor, |out| out.slot);
        }
    }

    /// executes one active slot, if any
    /// returns true if something was executed
    fn execute_one_active_slot(&mut self) -> bool {
        // get the next active slot
        let slot = self
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("active slot overflow in VM");

        let exec_target = match self.active_slots.get(&slot) {
            Some(b) => b.clone(), //TODO get rid of that clone
            None => return false,
        };

        // execute the slot
        let exec_out = self.execute_slot(slot, exec_target);

        // apply execution output to active state
        self.apply_active_execution_output(exec_out);

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

    /// executed a readonly request
    fn execute_readonly_request(&mut self, req: ReadOnlyExecutionRequest) {
        // TODO
    }

    /// main VM loop
    fn main_loop(&mut self) {
        loop {
            // read input queues
            let input_data = self.consume_input();

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
            for req in input_data.readonly_requests {
                self.execute_readonly_request(req);
            }

            // check if new data or requests arrived during the iteration
            let input_data = self
                .controller
                .input_data
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
                .loop_cv
                .wait_timeout(input_data, delay_until_next_slot.to_duration())
                .expect("VM main loop condition variable wait failed");
        }
    }
}
