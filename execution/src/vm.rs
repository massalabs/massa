use models::{Address, Block, BlockId, OperationType, Slot};
use std::fmt::Debug;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::debug;
use wasmer::{imports, Function, ImportObject, Instance, Module, Store, WasmerEnv};

use crate::sce_ledger::{SCELedger, SCELedgerChanges, SCELedgerStep};
use crate::ExecutionConfig;

/// Example API available to wasm code, aka "syscall".
fn foo(shared_env: &SharedExecutionContext, n: i32) -> i32 {
    n
}

/// API allowing a contract to call another.
fn call_address(shared_env: &SharedExecutionContext, addr: Address) {
    //if let Some(ledger_entry) = Arc::clone(&shared_env.0).lock().active_ledger.get(&addr) {}
    //TODO
}

#[derive(Clone)]
pub struct ExecutionStep {
    pub slot: Slot,
    // TODO add pos_seed for RNG seeding
    // TODO add pos_draws to list the draws (block and endorsement creators) for that slot
    pub block: Option<(BlockId, Block)>, // None if miss
}

#[derive(Clone)]
/// Stateful context, providing an execution context to host functions("syscalls").
pub struct ExecutionContext {
    pub ledger_step: SCELedgerStep,
}

#[derive(WasmerEnv, Clone)]
pub struct SharedExecutionContext(pub Arc<Mutex<ExecutionContext>>);

pub struct VM {
    cfg: ExecutionConfig,
    imports: ImportObject,
    store: Store,
    final_ledger: Arc<Mutex<SCELedger>>,
    step_history: VecDeque<(Slot, Option<BlockId>, SCELedgerChanges)>,
    current_execution_context: SharedExecutionContext,
}

impl VM {
    pub fn new(cfg: ExecutionConfig) -> VM {
        let store = Store::default();
        let final_ledger = Arc::new(Mutex::new(
            SCELedger::default(), // TODO bootstrap
        ));
        let current_execution_context =
            SharedExecutionContext(Arc::new(Mutex::new(ExecutionContext {
                ledger_step: SCELedgerStep {
                    final_ledger: final_ledger.clone(),
                    cumulative_history_changes: Default::default(),
                    caused_changes: Default::default(),
                },
            })));
        let imports = imports! {
            "env" => {
                "foo" => Function::new_native_with_env(&store, current_execution_context.clone(), foo),
            },
        };
        VM {
            cfg,
            imports,
            store,
            final_ledger,
            step_history: Default::default(),
            current_execution_context,
        }
    }

    /// runs an SCE-final execution step
    /// # Parameters
    ///   * step: execution step to run
    pub fn run_final_step(&mut self, step: ExecutionStep) {
        // check if step already in history front
        let opt_cached = {
            if let Some((slot, opt_block, ledger_changes)) = self.step_history.pop_front() {
                if slot != step.slot {
                    // slot mismatch
                    None
                } else {
                    match (&opt_block, &step.block) {
                        (None, None) => {
                            // matching miss
                            Some(ledger_changes)
                        }
                        (Some(b_id_hist), Some((b_id_step, b_step))) => {
                            if b_id_hist == b_id_step {
                                // matching block
                                Some(ledger_changes)
                            } else {
                                // block mismatch
                                None
                            }
                        }
                        (None, Some(_)) => None, // miss/block mismatch
                        (Some(_), None) => None, // block/miss mismatch
                    }
                }
            } else {
                None
            }
        };
        if let Some(cached) = opt_cached {
            // execution was already done, apply cached ledger changes to final ledger
            let mut final_ledger_guard = self.final_ledger.lock().unwrap();
            (*final_ledger_guard).apply_changes(&cached);
            return;
        }

        // nothing found in cache, or cache mismatch: reset history, run step and make it final
        // this should almost never happen, so the heavy clone() is OK
        self.reset_to_final();
        self.run_active_step(step.clone());
        self.run_final_step(step);
    }

    /// runs an SCE-active execution step
    /// # Parameters
    ///   * step: execution step to run
    pub fn run_active_step(&mut self, step: ExecutionStep) {
        // accumulate active ledger changes history
        let mut active_changes = SCELedgerChanges::default();
        for (step_slot, step_opt_block, step_changes) in self.step_history.iter() {
            active_changes.apply_changes(step_changes);
        }

        // setup active execution context
        {
            let mut exec_context_guard = self.current_execution_context.0.lock().unwrap();
            // clear caused changes
            (*exec_context_guard).ledger_step.caused_changes.0.clear();
            // accumulate history
            (*exec_context_guard).ledger_step.cumulative_history_changes = active_changes;
        }

        // TODO metering (ex: per-block gas limit)

        // run implicit and async calls
        //TODO

        // run explicit calls within the block
        if let Some((block_id, block)) = step.block {
            // get block creator addr
            // TODO remove unwrap
            let block_creator_addr =
                Address::from_public_key(&block.header.content.creator).unwrap();

            let mut remaining_block_gas = self.cfg.max_gas_per_block;

            // run all operations
            for (op_idx, operation) in block.operations.into_iter().enumerate() {
                let (module, max_gas, coins, gas_price) = if let OperationType::ExecuteSC {
                    data,
                    max_gas,
                    coins,
                    gas_price,
                } = operation.content.op
                {
                    // found an ExecuteSC operation
                    // TODO clear this up !
                    {
                        // get execution context guard
                        let mut exec_context_guard =
                            self.current_execution_context.0.lock().unwrap();

                        // get sender address
                        // TODO remove unwrap
                        let sender_addr =
                            Address::from_public_key(&operation.content.sender_public_key).unwrap();

                        // credit the sender with "coins"
                        // we do it early because otherwise the block creator can make it cancellable (out of gas)
                        // but the amount was already spend on the CSS side.
                        // This will be corrected with the single-ledger version of the smart contract system.
                        // TODO do not ignore result
                        let _result = (*exec_context_guard).ledger_step.set_balance_delta(
                            sender_addr,
                            coins,
                            true,
                        );

                        // check remaining per-block gas
                        match remaining_block_gas.checked_sub(max_gas) {
                            Some(remaining) => remaining_block_gas = remaining,
                            None => continue,
                        }

                        // credit the block creator with max_gas*gas_price
                        // TODO consider dispatching with edorsers/endorsed as well
                        // TODO remove unwrap()
                        // TODO do not ignore result
                        let _result = (*exec_context_guard).ledger_step.set_balance_delta(
                            block_creator_addr,
                            gas_price.checked_mul_u64(max_gas).unwrap(),
                            true,
                        );

                        // drop execution context guard
                    }

                    // parse operation bytecode
                    match Module::from_binary(&self.store, &data) {
                        Ok(module) => (module, max_gas, coins, gas_price),
                        Err(err) => {
                            // parsing failed
                            debug!(
                                "failed parsing bytecode in operation index {} in block {}: {}",
                                op_idx, block_id, err
                            );
                            continue;
                        }
                    }
                } else {
                    // not an ExecuteSC operations
                    continue;
                };

                // a module was successfully parsed and balances are ready

                // run the module
                // TODO metering
                // TODO find a way to provide context info:
                //    gas_price, max_gas, coins, block etc...
                let instance = Instance::new(&module, &self.imports).unwrap();
                let program = instance
                    .exports
                    .get_function("main")
                    .unwrap()
                    .native::<(), ()>()
                    .unwrap();
                match program.call() {
                    Ok(_rets) => {
                        // TODO check what to do with the return values. Probably nothing
                    }
                    Err(err) => {
                        // TODO figure out a way to cancel all the effects of the call
                        debug!(
                            "failed running bytecode in operation index {} in block {}: {}",
                            op_idx, block_id, err
                        );
                    }
                }
            }
        }
    }

    /// resets the VM to its latest final state
    pub fn reset_to_final(&mut self) {
        self.step_history.clear();
    }
}
