use std::sync::{Arc, Mutex};

use crate::error::bootstrap_file_error;
use crate::interface_impl::InterfaceImpl;
use crate::sce_ledger::{FinalLedger, SCELedger, SCELedgerChanges};
use crate::types::{ExecutionContext, ExecutionStep, StepHistory, StepHistoryItem};
use crate::{ExecutionError, ExecutionSettings};
use assembly_simulator::Interface;
use massa_models::address::AddressHashMap;
use massa_models::{Address, Amount, BlockId, OperationType, Slot};
use tracing::debug;

pub(crate) struct VM {
    _cfg: ExecutionSettings,
    step_history: StepHistory,
    execution_interface: Box<dyn Interface>,
    execution_context: Arc<Mutex<ExecutionContext>>,
}

impl VM {
    pub fn new(
        cfg: ExecutionSettings,
        thread_count: u8,
        ledger_bootstrap: Option<(SCELedger, Slot)>,
    ) -> Result<VM, ExecutionError> {
        let (ledger_bootstrap, ledger_slot) =
            if let Some((ledger_bootstrap, ledger_slot)) = ledger_bootstrap {
                // bootstrap from snapshot
                (ledger_bootstrap, ledger_slot)
            } else {
                // not bootstrapping: load initial SCE ledger from file
                let ledger_slot = Slot::new(0, thread_count.saturating_sub(1)); // last genesis block
                let ledgger_balances = serde_json::from_str::<AddressHashMap<Amount>>(
                    &std::fs::read_to_string(&cfg.initial_sce_ledger_path)
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
            _cfg: cfg,
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
    /// # Parameters
    ///   * step: execution step to run
    pub(crate) fn run_final_step(&mut self, step: &ExecutionStep) {
        // check if that step was already executed as the earliest active step
        // if so, pop it and apply it to the final ledger
        if let Some(cached) = self.pop_cached_step(step) {
            // execution was already done, apply cached ledger changes to final ledger
            let mut context = self.execution_context.lock().unwrap();
            let mut ledger_step = &mut (*context).ledger_step;
            ledger_step
                .final_ledger_slot
                .ledger
                .apply_changes(&cached.ledger_changes);
            ledger_step.final_ledger_slot.slot = step.slot;
            return;
        }

        // nothing found in cache, or cache mismatch: reset history, run step and make it final
        self.step_history.clear();
        self.run_active_step(step);

        // now, the result of the active run should be the sole element of the active step history
        // retrieve its result and apply it to the final ledger
        if let Some(cached) = self.pop_cached_step(step) {
            // execution is done, apply cached ledger changes to final ledger
            let mut context = self.execution_context.lock().unwrap();
            let mut ledger_step = &mut (*context).ledger_step;
            ledger_step
                .final_ledger_slot
                .ledger
                .apply_changes(&cached.ledger_changes);
            ledger_step.final_ledger_slot.slot = step.slot;
            return;
        } else {
            panic!("result of final step execution unavailable");
        }
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

    /// Prepare (update) the shared context before the new operation
    /// returns a snapshot copy of the current caused ledger changes
    /// TODO: do not ignore the results
    /// TODO consider dispatching with edorsers/endorsed as well
    fn prepare_context(
        &self,
        sender: Address,
        gas_price: Amount,
        max_gas: u64,
        coins: Amount,
        block_creator_addr: Address,
        block_id: BlockId,
        slot: Slot,
    ) -> SCELedgerChanges {
        let mut context = self.execution_context.lock().unwrap();

        // credit the sender with "coins"
        let _result = context.ledger_step.set_balance_delta(sender, coins, true);

        // credit the block creator with max_gas*gas_price
        let _result = context.ledger_step.set_balance_delta(
            block_creator_addr,
            gas_price.saturating_mul_u64(max_gas),
            true,
        );

        // fill context for execution
        context.gas_price = gas_price;
        context.max_gas = max_gas;
        context.coins = coins;
        context.slot = slot;
        context.opt_block_id = Some(block_id);
        context.opt_block_creator_addr = Some(block_creator_addr);
        context.call_stack = vec![sender].into();
        context.ledger_step.caused_changes.clone()
    }

    /// runs an SCE-active execution step
    ///
    /// 1. Get step history (cache of final ledger changes by slot and block_id history)
    /// 2. clear caused changes
    /// 3. accumulated step history
    /// 4. Execute each block of each operation
    ///
    /// # Parameters
    ///   * step: execution step to run
    pub(crate) fn run_active_step(&mut self, step: &ExecutionStep) {
        // accumulate active ledger changes history
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
                let (bytecode, max_gas, gas_price, coins, sender) =
                    if let OperationType::ExecuteSC {
                        data,
                        max_gas,
                        gas_price,
                        coins,
                        ..
                    } = &operation.content.op
                    {
                        let sender = Address::from_public_key(&operation.content.sender_public_key);
                        (data, *max_gas, *gas_price, *coins, sender)
                    } else {
                        continue;
                    };

                // Prepare context and save the Initial ledger changes before execution
                // The returned snapshot contains a copy of the initial coin credits
                // that will be popped back if bytecode execution fails in order to cancel its effects only
                let ledger_changes_backup = self.prepare_context(
                    sender,
                    gas_price,
                    max_gas,
                    coins,
                    block_creator_addr,
                    *block_id,
                    step.slot,
                );

                // run in the intepreter
                let run_result =
                    assembly_simulator::run(&bytecode, max_gas, &*self.execution_interface);
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

        let context = self.execution_context.lock().unwrap();
        // push step into history
        self.step_history.push_back(StepHistoryItem {
            slot: step.slot,
            opt_block_id,
            ledger_changes: context.ledger_step.caused_changes.clone(),
        });
    }

    pub fn reset_to_final(&mut self) {
        self.step_history.clear();
    }
}
