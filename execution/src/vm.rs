use models::{Address, BlockId};

use crate::types::{ExecutionContext, OperationSC, ExecutionStep, StepHistory};
use crate::sce_ledger::{SCELedger, SCELedgerChanges};
use crate::ExecutionConfig;

pub struct VM {
    _cfg: ExecutionConfig,
    step_history: StepHistory,
    context: ExecutionContext,
}

impl VM {
    pub fn new(_cfg: ExecutionConfig) -> VM {
        let ledger = SCELedger::default(); // TODO Bootstrap
        let context = ExecutionContext::new(ledger);
        VM {
            _cfg,
            step_history: Default::default(),
            context,
        }
    }

    /// runs an SCE-final execution step
    /// # Parameters
    ///   * step: execution step to run
    pub(crate) async fn run_final_step(&mut self, step: &ExecutionStep) {
        if let Some(cached) = self.is_already_done(step) {
            // execution was already done, apply cached ledger changes to final ledger
            let mut final_ledger_guard = self.context.ledger_step.final_ledger.lock().await;
            (*final_ledger_guard).apply_changes(&cached);
            return;
        }
        // nothing found in cache, or cache mismatch: reset history, run step and make it final
        // this should almost never happen, so the heavy step.clone() is OK
        self.step_history.clear();
        self.run_active_step(step).await;

        if let Some(cached) = self.is_already_done(step) {
            // execution was already done, apply cached ledger changes to final ledger
            // It should always happen
            let mut final_ledger_guard = self.context.ledger_step.final_ledger.lock().await;
            (*final_ledger_guard).apply_changes(&cached);
        }
    }

    fn is_already_done(&mut self, step: &ExecutionStep) -> Option<SCELedgerChanges> {
        // check if step already in history front
        if let Some((slot, opt_block, ledger_changes)) = self.step_history.pop_front() {
            if slot == step.slot {
                match (&opt_block, &step.block) {
                    (None, None) => Some(ledger_changes), // matching miss
                    (Some(b_id_hist), Some((b_id_step, _b_step))) => {
                        if b_id_hist == b_id_step {
                            Some(ledger_changes) // matching block
                        } else {
                            None // block mismatch
                        }
                    }
                    (None, Some(_)) => None, // miss/block mismatch
                    (Some(_), None) => None, // block/miss mismatch
                }
            } else {
                None // slot mismatch
            }
        } else {
            None
        }
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
    pub(crate) async fn run_active_step(&mut self, step: &ExecutionStep) {
        // accumulate active ledger changes history
        self.context.ledger_step.caused_changes.clear();
        self.context.ledger_step.cumulative_history_changes = SCELedgerChanges::from(self.step_history.clone());

        // run implicit and async calls
        // TODO

        // run explicit calls within the block (if the slot is not a miss)
        // note that total block gas is not checked, because currently Protocol makes the block invalid if it overflows gas
        let opt_block_id: Option<BlockId>;
        if let Some((block_id, block)) = &step.block {
            opt_block_id = Some(*block_id);

            // get block creator addr
            let block_creator_addr = Address::from_public_key(&block.header.content.creator).unwrap();
            // run all operations
            for (_op_idx, operation) in block.operations.clone().into_iter().enumerate() {
                let operation_sc = OperationSC::try_from(operation.content);
                if operation_sc.is_err() { // only fail if the operation cannot parse the sender address
                    continue;
                }
                let operation_sc = operation_sc.unwrap();

                // credit the sender with "coins"
                // TODO do not ignore result
                let _result = self.context.ledger_step.set_balance_delta(
                    operation_sc.sender,
                    operation_sc.coins,
                    true,
                ).await;

                // credit the block creator with max_gas*gas_price
                // TODO consider dispatching with edorsers/endorsed as well
                let _result = self.context.ledger_step.set_balance_delta(
                    block_creator_addr,
                    operation_sc.gas_price.checked_mul_u64(operation_sc.max_gas).unwrap(),
                    true,
                ).await;

                // Save the Initial ledger changes before execution
                // It contains a copy of the initial coin credits that will be popped back if bytecode execution fails in order to cancel its effects
                let _save_init_changes = self.context.ledger_step.caused_changes.clone();

                // fill context for execution
                // TODO provide more context: block, PoS seeds/draws, absolute time etc...
                self.context.gas_price = operation_sc.gas_price;
                self.context.max_gas = operation_sc.max_gas;
                self.context.coins = operation_sc.coins;
                self.context.slot = step.slot;
                self.context.opt_block_id = Some(*block_id);
                self.context.opt_block_creator_addr = Some(block_creator_addr);
                self.context.call_stack = Default::default();

                // TODO call the run module from the external execution project
                // Important, if execution fail or return an error, reset changes like this:
                //
                //
                // debug!(
                //     "failed running bytecode in operation index {} in block {}: {}",
                //     op_idx, block_id, err
                // );
                // // cancel the effects of execution only, pop back init_changes
                // let mut exec_context_guard = self.current_execution_context.0.lock().unwrap();
                // (*exec_context_guard).ledger_step.caused_changes = init_changes;
                //
            }
        } else {
            // There is no block for this step, miss
            opt_block_id = None;
        }

        // push step into history
        self.step_history.push_back((
            step.slot,
            opt_block_id,
            self.context.ledger_step.caused_changes.clone(),
        ))
    }
}
