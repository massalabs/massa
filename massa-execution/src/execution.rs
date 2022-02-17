use crate::config::VMConfig;
use crate::context::ExecutionContext;
use crate::event_store::EventStore;
use crate::interface_impl::InterfaceImpl;
use crate::types::{ExecutionOutput, ExecutionStackElement, ReadOnlyExecutionRequest};
use crate::ExecutionError;
use massa_ledger::{Applicable, FinalLedger, LedgerChanges, LedgerEntry, SetUpdateOrDelete};
use massa_models::{Address, BlockId, Operation, OperationType};
use massa_models::{Block, Slot};
use massa_sc_runtime::Interface;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, RwLock},
};
use tracing::debug;

macro_rules! context_guard {
    ($self:ident) => {
        $self
            .execution_context
            .lock()
            .expect("failed to acquire lock on execution context")
    };
}

/// structure holding consistent speculative and final execution states
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
        let execution_context = Arc::new(Mutex::new(ExecutionContext::new(
            final_ledger.clone(),
            Default::default(),
        )));

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
    /// TODO: do not do this anymore but allow the speculative ledger to lazily query any subentry
    /// by scanning through history from end to beginning
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

    /// execute an operation in the context of a block
    pub fn execute_operation(
        &mut self,
        operation: &Operation,
        block_creator_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process ExecuteSC operations only
        let (bytecode, max_gas, coins, gas_price) = match &operation.content.op {
            op @ OperationType::ExecuteSC {
                data,
                max_gas,
                coins,
                gas_price,
            } => (data, max_gas, coins, gas_price),
            _ => return Ok(()),
        };

        // get sender address
        let sender_addr = Address::from_public_key(&operation.content.sender_public_key);

        // get operation ID
        // TODO have operation_id contained in the Operation object in the future to avoid recomputation
        let operation_id = operation
            .get_operation_id()
            .expect("could not compute operation ID");

        // prepare the context
        let context_snapshot;
        {
            let context = context_guard!(self);

            // credit the producer of the block B with max_gas * gas_price parallel coins
            // note that errors are deterministic and do not cancel op execution
            let gas_fees = gas_price.saturating_mul_u64(*max_gas);
            if let Err(err) =
                context.transfer_parallel_coins(None, Some(block_creator_addr), gas_fees, false)
            {
                debug!(
                    "failed to credit block producer {} with {} gas fee coins: {}",
                    block_creator_addr, gas_fees, err
                );
            }

            // credit Op's sender with `coins` parallel coins
            // note that errors are deterministic and do not cancel op execution
            if let Err(err) =
                context.transfer_parallel_coins(None, Some(sender_addr), *coins, false)
            {
                debug!(
                    "failed to credit operation sender {} with {} operation coins: {}",
                    sender_addr, *coins, err
                );
            }

            // save a snapshot of the context state to restore it if the op fails to execute
            context_snapshot = context.get_snapshot();

            // prepare context for op execution
            context.gas_price = *gas_price;
            context.max_gas = *max_gas;
            context.stack = vec![ExecutionStackElement {
                address: sender_addr,
                coins: *coins,
                owned_addresses: vec![sender_addr],
            }];
            context.origin_operation_id = Some(operation_id);
        };

        // run the intepreter
        let run_result = massa_sc_runtime::run(bytecode, *max_gas, &*self.execution_interface);
        if let Err(err) = run_result {
            // there was an error during bytecode execution: cancel the effects of the execution
            let mut context = context_guard!(self);
            context.origin_operation_id = None;
            context.reset_to_snapshot(context_snapshot);
            return Err(ExecutionError::RuntimeError(format!(
                "bytecode execution error: {}",
                err
            )));
        }

        Ok(())
    }

    /// executes a full slot without causing any changes to the state,
    /// and yields an execution output
    pub fn execute_slot(&self, slot: Slot, opt_block: Option<(BlockId, Block)>) -> ExecutionOutput {
        // get optional block ID and creator address
        let (opt_block_id, opt_block_creator_addr) = opt_block
            .as_ref()
            .map(|(b_id, b)| (*b_id, Address::from_public_key(&b.header.content.creator)))
            .unzip();

        // accumulate previous active changes from history
        let previous_ledger_changes = self.get_accumulated_active_changes_at_slot(slot);

        // prepare execution context for the whole active slot
        let execution_context = ExecutionContext::new_active_slot(
            slot,
            opt_block_id,
            opt_block_creator_addr,
            previous_ledger_changes,
            self.final_ledger.clone(),
        );

        // note that here, some pre-operations (like crediting block producers) can be performed before the lock

        // set the execution context for slot execution
        *context_guard!(self) = execution_context;

        // note that here, async operations should be executed

        // check if there is a block at this slot
        if let (Some((block_id, block)), Some(block_creator_addr)) =
            (opt_block, opt_block_creator_addr)
        {
            // execute operations
            for (op_idx, operation) in block.operations.iter().enumerate() {
                if let Err(err) = self.execute_operation(operation, block_creator_addr) {
                    debug!(
                        "failed executing operation index {} in block {}: {}",
                        op_idx, block_id, err
                    );
                }
            }
        }

        // return the execution output
        context_guard!(self).take_execution_output()
    }

    /// execute a readonly request
    pub(crate) fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError> {
        // set the exec slot just after the latest executed active slot
        let slot = self
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow in readonly execution");

        // get previous changes
        let previous_ledger_changes = self.get_accumulated_active_changes_at_slot(slot);

        // create readonly execution context
        let execution_context = ExecutionContext::new_readonly(
            slot,
            req,
            previous_ledger_changes,
            self.final_ledger.clone(),
        );

        // set the execution context for execution
        *context_guard!(self) = execution_context;

        // run the intepreter
        massa_sc_runtime::run(&req.bytecode, req.max_gas, &*self.execution_interface)
            .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;

        // return the execution output
        Ok(context_guard!(self).take_execution_output())
    }

    /// gets a full ledger entry both at final and active states
    /// TODO: this can be heavily optimized, see comments
    ///
    /// # returns
    /// (final_entry, active_entry)
    pub fn get_full_ledger_entry(
        &self,
        addr: &Address,
    ) -> (Option<LedgerEntry>, Option<LedgerEntry>) {
        // get the full entry from the final ledger
        let final_entry = self
            .final_ledger
            .read()
            .expect("could not r-lock final ledger")
            .get_full_entry(addr);

        // get cumulative active changes and apply them
        // TODO there is a lot of overhead here: we only need to compute the changes for one entry and no need to clone it
        // also we should proceed backwards through history for performance
        let active_change = self
            .get_accumulated_active_changes_at_slot(self.active_cursor)
            .get(addr)
            .cloned();
        let active_entry = match (&final_entry, active_change) {
            (final_v, None) => final_v.clone(),
            (_, Some(SetUpdateOrDelete::Set(v))) => Some(v.clone()),
            (_, Some(SetUpdateOrDelete::Delete)) => None,
            (None, Some(SetUpdateOrDelete::Update(u))) => {
                let mut v = LedgerEntry::default();
                v.apply(u);
                Some(v)
            }
            (Some(final_v), Some(SetUpdateOrDelete::Update(u))) => {
                let mut v = final_v.clone();
                v.apply(u);
                Some(v)
            }
        };

        (final_entry, active_entry)
    }
}
