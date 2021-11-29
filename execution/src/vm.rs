use models::{Address, Amount, Block, BlockId, OperationType, Slot};
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
    pub max_gas: u64,
    pub coins: Amount,
    pub gas_price: Amount,
    pub slot: Slot,
    pub opt_block_id: Option<BlockId>,
    pub opt_block_creator_addr: Option<Address>,
    pub call_stack: Vec<Address>,
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
                max_gas: Default::default(),
                coins: Default::default(),
                gas_price: Default::default(),
                slot: Slot::new(0, 0),
                opt_block_id: Default::default(),
                opt_block_creator_addr: Default::default(),
                call_stack: Default::default(),
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
                        (Some(b_id_hist), Some((b_id_step, _b_step))) => {
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
        // this should almost never happen, so the heavy step.clone() is OK
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
        for (_step_slot, _step_opt_block, step_changes) in self.step_history.iter() {
            active_changes.apply_changes(step_changes);
        }

        // setup active execution context
        {
            let mut exec_context_guard = self.current_execution_context.0.lock().unwrap();
            // clear caused changes
            (*exec_context_guard).ledger_step.caused_changes.0.clear();
            // accumulated step history
            (*exec_context_guard).ledger_step.cumulative_history_changes = active_changes;
            // TODO add more info in the exec_context: slot, PoS draws, optional block, call stack etc...
            //      if possible prefer Arc/Mutex sharing to copying full blocks
        }

        // run implicit and async calls
        //TODO

        // run explicit calls within the block (if the slot is not a miss)
        // note that total block gas is not checked, because currently Protocol makes the block invalid if it overflows gas
        let opt_block_id: Option<BlockId>;
        if let Some((block_id, block)) = step.block {
            opt_block_id = Some(block_id);

            // get block creator addr
            // TODO remove unwrap
            let block_creator_addr =
                Address::from_public_key(&block.header.content.creator).unwrap();

            // run all operations
            for (op_idx, operation) in block.operations.into_iter().enumerate() {
                let (module, max_gas, coins, gas_price, init_changes, sender_addr) =
                    if let OperationType::ExecuteSC {
                        data,
                        max_gas,
                        coins,
                        gas_price,
                    } = operation.content.op
                    {
                        // get sender address
                        // TODO remove unwrap
                        let sender_addr =
                            Address::from_public_key(&operation.content.sender_public_key).unwrap();

                        // found an ExecuteSC operation
                        let init_changes = {
                            // get execution context guard
                            let mut exec_context_guard =
                                self.current_execution_context.0.lock().unwrap();

                            // credit the sender with "coins"
                            // TODO do not ignore result
                            let _result = (*exec_context_guard).ledger_step.set_balance_delta(
                                sender_addr,
                                coins,
                                true,
                            );

                            // credit the block creator with max_gas*gas_price
                            // TODO consider dispatching with edorsers/endorsed as well
                            // TODO remove unwrap()
                            // TODO do not ignore result
                            let _result = (*exec_context_guard).ledger_step.set_balance_delta(
                                block_creator_addr,
                                gas_price.checked_mul_u64(max_gas).unwrap(),
                                true,
                            );

                            // save caused changes
                            (*exec_context_guard).ledger_step.caused_changes.clone()

                            // drop execution context guard
                        };

                        // parse operation bytecode
                        match Module::from_binary(&self.store, &data) {
                            Ok(module) => {
                                (module, max_gas, coins, gas_price, init_changes, sender_addr)
                            }
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
                // init_changes contains a copy of the initial coin credits that will be popped back if bytecode execution fails in order to cancel its effects

                // fill context for execution
                {
                    let mut exec_context_guard = self.current_execution_context.0.lock().unwrap();
                    (*exec_context_guard).gas_price = gas_price;
                    (*exec_context_guard).max_gas = max_gas;
                    (*exec_context_guard).coins = coins;
                    (*exec_context_guard).slot = step.slot;
                    (*exec_context_guard).opt_block_id = Some(block_id);
                    (*exec_context_guard).opt_block_creator_addr = Some(block_creator_addr);
                    (*exec_context_guard).call_stack = vec![sender_addr];
                    // TODO provide more context:
                    //   block, PoS seeds/draws, absolute time etc...
                }

                // TODO add mem limit and metering
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
                        debug!(
                            "failed running bytecode in operation index {} in block {}: {}",
                            op_idx, block_id, err
                        );

                        // cancel the effects of execution only, pop back init_changes
                        let mut exec_context_guard =
                            self.current_execution_context.0.lock().unwrap();
                        (*exec_context_guard).ledger_step.caused_changes = init_changes;
                    }
                }
            }
        } else {
            // miss
            opt_block_id = None;
        }

        // push step into history
        {
            let exec_context_guard = self.current_execution_context.0.lock().unwrap();
            self.step_history.push_back((
                step.slot,
                opt_block_id,
                (*exec_context_guard).ledger_step.caused_changes.clone(),
            ))
        }
    }

    /// resets the VM to its latest final state
    pub fn reset_to_final(&mut self) {
        self.step_history.clear();
    }
}
