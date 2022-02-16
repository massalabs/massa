use crate::config::VMConfig;
use crate::context::ExecutionContext;
use crate::interface_impl::InterfaceImpl;
use crate::types::{ExecutionOutput, ReadOnlyExecutionRequest};
use crate::ExecutionError;
use crate::{event_store::EventStore, speculative_ledger::SpeculativeLedger};
use massa_ledger::{Applicable, FinalLedger, LedgerChanges};
use massa_models::BlockId;
use massa_models::{Block, Slot};
use massa_sc_runtime::Interface;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, RwLock},
};

pub struct ExecutionState {
    // VM config
    pub config: VMConfig,
    // active execution output history
    pub active_history: VecDeque<ExecutionOutput>,
    // active execution cursor
    pub active_cursor: Slot,
    // final execution cursor
    pub final_cursor: Slot,
    // final events
    pub final_events: EventStore,
    // final ledger
    pub final_ledger: Arc<RwLock<FinalLedger>>,
    // execution context
    pub execution_context: Arc<Mutex<ExecutionContext>>,
    // execution interface
    pub execution_interface: Box<dyn Interface>,
}

impl ExecutionState {
    /// create a new execution state
    pub fn new(config: VMConfig, final_ledger: Arc<RwLock<FinalLedger>>) -> ExecutionState {
        // get last final slot from final ledger
        let last_final_slot = final_ledger
            .read()
            .expect("could not r-lock final ledger")
            .slot;

        // init execution context
        let execution_context = Arc::new(Mutex::new(ExecutionContext::new(final_ledger.clone())));

        // Instantiate the interface used by the assembly simulator.
        let execution_interface = Box::new(InterfaceImpl::new(
            config.clone(),
            execution_context.clone(),
        ));

        // build execution state
        ExecutionState {
            config,
            final_ledger,
            execution_context,
            execution_interface,
            active_history: Default::default(),
            final_events: Default::default(),
            active_cursor: last_final_slot,
            final_cursor: last_final_slot,
        }
    }

    /// applies an execution output to the final state
    pub fn apply_final_execution_output(&mut self, exec_out: ExecutionOutput) {
        // apply final ledger changes
        self.final_ledger
            .write()
            .expect("could not lock final ledger for writing")
            .settle_slot(exec_out.slot, exec_out.ledger_changes);
        self.final_cursor = exec_out.slot;

        // update active cursor
        if self.active_cursor < self.final_cursor {
            self.active_cursor = self.final_cursor;
        }

        // save generated events to final store
        self.final_events.extend(exec_out.events);
    }

    /// applies an execution output to the active state
    pub fn apply_active_execution_output(&mut self, exec_out: ExecutionOutput) {
        // update active cursor
        self.active_cursor = exec_out.slot;

        // add execution output to history
        self.active_history.push_back(exec_out);
    }

    /// clear execution history
    pub fn clear_history(&mut self) {
        // clear history
        self.active_history.clear();

        // reset active cursor
        self.active_cursor = self.final_cursor;
    }

    /// truncates active slots at the first mismatch
    /// between the active execution output history and the planned active_slots
    pub fn truncate_history(&mut self, active_slots: &HashMap<Slot, Option<(BlockId, Block)>>) {
        // find mismatch point (included)
        let mut truncate_at = None;
        for (hist_index, exec_output) in self.active_history.iter().enumerate() {
            let found_block_id = active_slots
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
            self.active_history.truncate(truncate_at);
            self.active_cursor = self
                .active_history
                .back()
                .map_or(self.final_cursor, |out| out.slot);
        }
    }

    /// returns the speculative ledger at the entrance of a given history slot
    /// warning: only use in the main loop because the lock on the final ledger
    /// at the base of the returned SpeculativeLedger is not held
    pub fn get_accumulated_active_changes_at_slot(&self, slot: Slot) -> LedgerChanges {
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
        let mut accumulated_changes = LedgerChanges::default();
        for previous_output in &self.active_history {
            if previous_output.slot >= slot {
                break;
            }
            accumulated_changes.apply(previous_output.ledger_changes.clone());
        }

        accumulated_changes
    }

    /// executes a full slot without causing any changes to the state,
    /// and yields an execution output
    pub fn execute_slot(&self, slot: Slot, opt_block: Option<(BlockId, Block)>) -> ExecutionOutput {
        // get the speculative ledger
        let previous_ledger_changes = self.get_accumulated_active_changes_at_slot(slot);
        let ledger = SpeculativeLedger::new(self.final_ledger.clone(), previous_ledger_changes);

        // TODO init context

        // TODO async executions

        let mut out_block_id = None;
        if let Some((block_id, block)) = opt_block {
            out_block_id = Some(block_id);

            //TODO execute block elements
        }

        ExecutionOutput {
            slot,
            block_id: out_block_id,
            ledger_changes: ledger.into_added_changes(),
        }
    }

    /// executed a readonly request
    pub(crate) fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError> {
        // execute at the slot just after the latest executed active slot
        let slot = self
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow in readonly execution");

        // get the speculative ledger
        let previous_ledger_changes = self.get_accumulated_active_changes_at_slot(slot);
        let ledger = SpeculativeLedger::new(self.final_ledger.clone(), previous_ledger_changes);

        // TODO execute ReadOnlyExecutionRequest at slot with context req

        //TODO send execution result back through req.result_sender
        Ok(ExecutionOutput {
            slot,
            block_id: None,
            events: TODO,
            ledger_changes: TODO,
        })
    }
}
