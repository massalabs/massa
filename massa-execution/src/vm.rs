use std::mem;
use std::sync::{Arc, Mutex};

use crate::error::bootstrap_file_error;
use crate::interface_impl::InterfaceImpl;
use crate::sce_ledger::{FinalLedger, SCELedger, SCELedgerChanges};
use crate::types::{ExecutionContext, ExecutionData, ExecutionStep, StepHistory, StepHistoryItem};
use crate::{config::ExecutionConfigs, ExecutionError};
use assembly_simulator::Interface;
use massa_models::address::AddressHashMap;
use massa_models::{
    execution::{ExecuteReadOnlyResponse, ReadOnlyResult},
    Address, Amount, BlockId, Slot,
};
use massa_signature::{derive_public_key, generate_random_private_key};
use tokio::sync::oneshot;
use tracing::debug;

pub(crate) struct VM {
    step_history: StepHistory,
    execution_interface: Box<dyn Interface>,
    execution_context: Arc<Mutex<ExecutionContext>>,
}

impl VM {
    pub fn new(
        cfg: ExecutionConfigs,
        ledger_bootstrap: Option<(SCELedger, Slot)>,
    ) -> Result<VM, ExecutionError> {
        let (ledger_bootstrap, ledger_slot) =
            if let Some((ledger_bootstrap, ledger_slot)) = ledger_bootstrap {
                // bootstrap from snapshot
                (ledger_bootstrap, ledger_slot)
            } else {
                // not bootstrapping: load initial SCE ledger from file
                let ledger_slot = Slot::new(0, cfg.thread_count.saturating_sub(1)); // last genesis block
                let ledgger_balances = serde_json::from_str::<AddressHashMap<Amount>>(
                    &std::fs::read_to_string(&cfg.settings.initial_sce_ledger_path)
                        .map_err(bootstrap_file_error!("loading", cfg))?,
                )
                .map_err(bootstrap_file_error!("parsing", cfg))?;
                let ledger_bootstrap = SCELedger::from_balances_map(ledgger_balances);
                (ledger_bootstrap, ledger_slot)
            };

        // Context shared between VM and the interface provided to the assembly simulator.
        let execution_context = Arc::new(Mutex::new(ExecutionContext::new(
            ledger_bootstrap,
            ledger_slot,
        )));

        // Instantiate the interface used by the assembly simulator.
        let execution_interface = Box::new(InterfaceImpl::new(Arc::clone(&execution_context)));

        Ok(VM {
            step_history: Default::default(),
            execution_interface,
            execution_context,
        })
    }

    // clone bootstrap state (final ledger and slot)
    pub fn get_bootstrap_state(&self) -> FinalLedger {
        self.execution_context
            .lock()
            .unwrap()
            .ledger_step
            .final_ledger_slot
            .clone()
    }

    /// runs an SCE-final execution step
    /// See https://github.com/massalabs/massa/wiki/vm_ledger_interaction
    ///
    /// # Parameters
    ///   * step: execution step to run
    pub(crate) fn run_final_step(&mut self, step: ExecutionStep) {
        // check if that step was already executed as the earliest active step
        let history_item = if let Some(cached) = self.pop_cached_step(&step) {
            // if so, pop it
            cached
        } else {
            // otherwise, clear step history an run it again explicitly
            self.step_history.clear();
            self.run_step_internal(&step)
        };

        // apply ledger changes to final ledger
        let mut context = self.execution_context.lock().unwrap();
        let mut ledger_step = &mut (*context).ledger_step;
        ledger_step
            .final_ledger_slot
            .ledger
            .apply_changes(&history_item.ledger_changes);
        ledger_step.final_ledger_slot.slot = step.slot;
    }

    /// check if step already at history front, if so, pop it
    fn pop_cached_step(&mut self, step: &ExecutionStep) -> Option<StepHistoryItem> {
        let found = if let Some(StepHistoryItem {
            slot, opt_block_id, ..
        }) = self.step_history.front()
        {
            if *slot == step.slot {
                match (&opt_block_id, &step.block) {
                    // matching miss
                    (None, None) => true,

                    // matching block
                    (Some(b_id_hist), Some((b_id_step, _b_step))) => (b_id_hist == b_id_step),

                    // miss/block mismatch
                    (None, Some(_)) => false,

                    // block/miss mismatch
                    (Some(_), None) => false,
                }
            } else {
                false // slot mismatch
            }
        } else {
            false // no item
        };

        // rerturn the step if found
        if found {
            self.step_history.pop_front()
        } else {
            None
        }
    }

    /// clear the execution context
    fn clear_and_update_context(&self) {
        let mut context = self.execution_context.lock().unwrap();
        context.ledger_step.caused_changes.clear();
        context.ledger_step.cumulative_history_changes =
            SCELedgerChanges::from(self.step_history.clone());
    }

    /// Prepares (updates) the shared context before the new operation.
    /// Returns a snapshot of the current caused ledger changes.
    /// See https://github.com/massalabs/massa/wiki/vm_ledger_interaction
    /// TODO: do not ignore the results
    /// TODO: consider dispatching gas fees with edorsers/endorsees as well
    fn prepare_context(
        &self,
        data: &ExecutionData,
        block_creator_addr: Address,
        block_id: BlockId,
        slot: Slot,
    ) -> SCELedgerChanges {
        let mut context = self.execution_context.lock().unwrap();
        // make context.ledger_step credit Op's sender with Op.coins in the SCE ledger
        let _result = context
            .ledger_step
            .set_balance_delta(data.sender_address, data.coins, true);

        // make context.ledger_step credit the producer of the block B with Op.max_gas * Op.gas_price in the SCE ledger
        let _result = context.ledger_step.set_balance_delta(
            block_creator_addr,
            data.gas_price.saturating_mul_u64(data.max_gas),
            true,
        );

        // fill context for execution
        context.gas_price = data.gas_price;
        context.max_gas = data.max_gas;
        context.coins = data.coins;
        context.slot = slot;
        context.opt_block_id = Some(block_id);
        context.opt_block_creator_addr = Some(block_creator_addr);
        context.call_stack = vec![data.sender_address].into();
        context.ledger_step.caused_changes.clone()
    }

    /// Run code in read-only mode
    pub(crate) fn run_read_only(
        &self,
        slot: Slot,
        max_gas: u64,
        simulated_gas_price: Amount,
        bytecode: Vec<u8>,
        address: Option<Address>,
        result_sender: oneshot::Sender<ExecuteReadOnlyResponse>,
    ) {
        // Reset active ledger changes history
        self.clear_and_update_context();

        {
            let mut context = self.execution_context.lock().unwrap();

            // Set the call stack, using the provided address, or a random one.
            let address = address.unwrap_or_else(|| {
                let private_key = generate_random_private_key();
                let public_key = derive_public_key(&private_key);
                Address::from_public_key(&public_key)
            });
            context.call_stack = vec![address].into();

            // Set the max gas.
            context.max_gas = max_gas;

            // Set the simulated gas price.
            context.gas_price = simulated_gas_price;
        }

        // run in the intepreter
        let run_result = assembly_simulator::run(&bytecode, max_gas, &*self.execution_interface);

        // Send result back.
        let execution_response = ExecuteReadOnlyResponse {
            executed_at: slot,
            // TODO: specify result.
            result: run_result.map_or_else(
                |_| ReadOnlyResult::Error("Failed to run in read-only mode".to_string()),
                |_| ReadOnlyResult::Ok,
            ),
            // TODO: integrate with output events.
            output_events: None,
        };
        if result_sender.send(execution_response).is_err() {
            debug!("Execution: could not send ExecuteReadOnlyResponse.");
        }

        // Note: changes are not applied to the ledger.
    }

    /// Runs an active step
    /// See https://github.com/massalabs/massa/wiki/vm_ledger_interaction
    ///
    /// 1. Get step history (cache of final ledger changes by slot and block_id history)
    /// 2. clear caused changes
    /// 3. accumulated step history
    /// 4. Execute each block of each operation
    ///
    /// # Parameters
    ///   * step: execution step to run
    fn run_step_internal(&mut self, step: &ExecutionStep) -> StepHistoryItem {
        // reset active ledger changes history
        self.clear_and_update_context();

        // run implicit and async calls
        // TODO

        // run explicit calls within the block (if the slot is not a miss)
        // note that total block gas is not checked, because currently Protocol makes the block invalid if it overflows gas
        let opt_block_id: Option<BlockId>;
        if let Some((block_id, block)) = &step.block {
            opt_block_id = Some(*block_id);

            // get block creator addr
            let block_creator_addr = Address::from_public_key(&block.header.content.creator);
            // run all operations
            for (op_idx, operation) in block.operations.iter().enumerate() {
                // process ExecuteSC operations only
                let execution_data = match ExecutionData::try_from(operation) {
                    Ok(data) => data,
                    _ => continue,
                };

                // Prepare context and save the initial ledger changes before execution.
                // The returned snapshot takes into account the initial coin credits.
                // This snapshot will be popped back if bytecode execution fails.
                let ledger_changes_backup =
                    self.prepare_context(&execution_data, block_creator_addr, *block_id, step.slot);

                // run in the intepreter
                let run_result = assembly_simulator::run(
                    &execution_data.bytecode,
                    execution_data.max_gas,
                    &*self.execution_interface,
                );
                if let Err(err) = run_result {
                    debug!(
                        "failed running bytecode in operation index {} in block {}: {}",
                        op_idx, block_id, err
                    );
                    // cancel the effects of execution only, pop back init_changes
                    let mut context = self.execution_context.lock().unwrap();
                    context.ledger_step.caused_changes = ledger_changes_backup;
                }
            }
        } else {
            // There is no block for this step, miss
            opt_block_id = None;
        }

        // generate history item
        let mut context = self.execution_context.lock().unwrap();
        StepHistoryItem {
            slot: step.slot,
            opt_block_id,
            ledger_changes: mem::take(&mut context.ledger_step.caused_changes),
        }
    }

    /// runs an SCE-active execution step
    /// See https://github.com/massalabs/massa/wiki/vm_ledger_interaction
    ///
    /// # Parameters
    ///   * step: execution step to run
    pub(crate) fn run_active_step(&mut self, step: ExecutionStep) {
        // run step
        let history_item = self.run_step_internal(&step);

        // push step into history
        self.step_history.push_back(history_item);
    }

    pub fn reset_to_final(&mut self) {
        self.step_history.clear();
    }
}
