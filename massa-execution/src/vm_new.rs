use massa_models::{
    timeslots::{get_block_slot_timestamp, get_latest_block_slot_at_timestamp},
    Slot,
};
use massa_time::MassaTime;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Condvar, Mutex},
};
use tracing::info;

use massa_models::BlockId;

pub struct VMConfig {
    thread_count: u8,
    cursor_delay: MassaTime,
    clock_compensation: i64,
    genesis_timestamp: MassaTime,
    t0: MassaTime,
}

pub struct VMBootstrapData {}

#[derive(Default)]
pub struct VMInputData {
    stop: bool,
    blockclique_changed: bool,
    finalized_blocks: HashMap<Slot, BlockId>,
    blockclique: HashMap<Slot, BlockId>,
}

pub struct VMController {
    loop_cv: Condvar,
    input_data: Mutex<VMInputData>,
}

pub struct VMManager {
    controller: Arc<VMController>,
    thread_handle: std::thread::JoinHandle<()>,
}

impl VMManager {
    pub fn stop(self) {
        info!("stopping VM controller...");
        {
            let mut input_wlock = self
                .controller
                .input_data
                .lock()
                .expect("could not w-lock VM input data");
            input_wlock.stop = true;
            input_wlock.blockclique_changed = true;
            self.controller.loop_cv.notify_one();
        }
        self.thread_handle
            .join()
            .expect("VM controller thread panicked");
        info!("VM controller stopped");
    }

    pub fn get_controller(&self) -> Arc<VMController> {
        self.controller.clone()
    }
}

pub fn start_vm(config: VMConfig, bootstrap: Option<VMBootstrapData>) -> VMManager {
    let controller = Arc::new(VMController {
        loop_cv: Condvar::new(),
        input_data: Mutex::new(VMInputData {
            blockclique_changed: true,
            ..Default::default()
        }),
    });

    let ctl = controller.clone();
    let thread_handle = std::thread::spawn(move || {
        VMThread::new(config, ctl, bootstrap).main_loop();
    });

    VMManager {
        controller,
        thread_handle,
    }
}

struct ExecutionOutput {
    slot: Slot,
    block_id: Option<BlockId>,
    //TODO ledger_changes
    //TODO event_store
}

struct VMThread {
    // VM config
    config: VMConfig,
    // VM data exchange controller
    controller: Arc<VMController>,
    // map of SCE-final blocks not executed yet
    sce_finals: HashMap<Slot, Option<BlockId>>,
    // last SCE final slot in sce_finals list
    last_sce_final: Slot,
    // map of CSS-final but non-SCE-final blocks
    remaining_css_finals: HashMap<Slot, BlockId>,
    // last blockclique
    blockclique: HashMap<Slot, BlockId>,
    // map of active slots
    active_slots: HashMap<Slot, Option<BlockId>>,
    // highest active slot
    last_active_slot: Slot,
    // final execution cursor
    final_cursor: Slot,
    // active execution cursor
    active_cursor: Slot,
    // execution output history
    execution_history: VecDeque<ExecutionOutput>,
}

impl VMThread {
    fn new(
        config: VMConfig,
        controller: Arc<VMController>,
        _bootstrap: Option<VMBootstrapData>,
    ) -> Self {
        // TODO bootstrap
        VMThread {
            controller,
            sce_finals: Default::default(),
            last_sce_final: Slot::new(0, config.thread_count.saturating_sub(1)),
            remaining_css_finals: Default::default(),
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
    fn update_final_slots(&mut self, new_css_finals: HashMap<Slot, BlockId>) {
        // return if empty
        if new_css_finals.is_empty() {
            return;
        }

        // add them to pending css finals
        self.remaining_css_finals.extend(new_css_finals);

        // get maximal css-final slot
        let max_css_final_slot = self
            .remaining_css_finals
            .iter()
            .max_by_key(|(s, _b_id)| *s)
            .map(|(s, _b_id)| *s)
            .expect("expected remaining_css_finals to be non-empty");

        // detect SCE-final slots
        let mut slot = self.last_sce_final;
        while slot < max_css_final_slot {
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("final slot overflow in VM");

            // pop slot from remaining CSS finals
            if let Some(block_id) = self.remaining_css_finals.remove(&slot) {
                // CSS-final block found at slot: add block to to sce_finals
                self.sce_finals.insert(slot, Some(block_id));
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
    fn update_active_slots(&mut self, new_blockclique: Option<HashMap<Slot, BlockId>>) {
        // update blockclique if changed
        if let Some(blockclique) = new_blockclique {
            self.blockclique = blockclique;
        }

        // get last active slot, if any
        let current_active_slot = self.get_end_active_slot();

        // reset active slots
        self.active_slots = HashMap::new();
        self.last_active_slot = self.last_sce_final;

        // if no active slot yet => keep the active_slots empty
        let current_active_slot = match current_active_slot {
            Some(s) => s,
            None => return,
        };

        // recompute non-SCE-final slot sequence
        let mut slot = self.last_sce_final;
        while slot < current_active_slot {
            slot = slot
                .get_next_slot(self.config.thread_count)
                .expect("active slot overflow in VM");
            if let Some(block_id) = self.remaining_css_finals.get(&slot) {
                // found in remaining_css_finals
                self.active_slots.insert(slot, Some(*block_id));
            } else if let Some(block_id) = self.blockclique.get(&slot) {
                // found in blockclique
                self.active_slots.insert(slot, Some(*block_id));
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

        // get the next one
        let slot = self
            .final_cursor
            .get_next_slot(self.config.thread_count)
            .expect("final slot overflow in VM");
        let block_id = *self
            .sce_finals
            .get(&slot)
            .expect("the SCE final slot list skipped a slot");

        // update final cursor
        self.final_cursor = slot;

        // check if the final slot is cached at the front of the speculative execution history
        if let Some(exec_out) = self.execution_history.pop_front() {
            if exec_out.slot == slot && exec_out.block_id == block_id {
                // speculative execution front result matches what we wnat to compute

                // TODO apply exec_out to final state

                return true;
            }
        }

        // speculative cache mismatch

        // clear the speculative execution output cache completely
        self.execution_history.clear();
        self.active_cursor = self.final_cursor;

        // TODO execute
        // TODO apply exec_out to final state

        return true;
    }

    /// truncates active slots at the fitst mismatch
    /// between the active execution output history and the planned active_slots
    fn truncate_history(&mut self) {
        // find mismatch point (included)
        let mut truncate_at = None;
        for (hist_index, exec_output) in self.execution_history.iter().enumerate() {
            if self.active_slots.get(&exec_output.slot) == Some(&exec_output.block_id) {
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
        let to_execute = match self.active_slots.get(&slot) {
            Some(b) => b,
            None => return false,
        };

        // update active cursor
        self.active_cursor = slot;

        // TODO execute

        // TODO push_back result into history

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

            // TODO execute readonly requests

            // check if data changed during the iteration
            let ctl = self.controller;
            let input_data = ctl.input_data.lock().expect("could not lock VM input data");
            if input_data.stop {
                break;
            }
            if input_data.blockclique_changed {
                continue;
            }

            // compute when the next slot is
            let delay_until_next_slot = self.get_time_until_next_active_slot();
            if delay_until_next_slot == 0.into() {
                // next slot is right now
                continue;
            }

            // wait for change or for next slot
            let _ = ctl
                .loop_cv
                .wait_timeout(input_data, delay_until_next_slot.to_duration())
                .expect("VM main loop condition variable wait failed");
        }
    }
}
