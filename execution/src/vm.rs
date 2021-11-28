use massa_hash::hash::Hash;
use models::hhasher::HHashMap;
use models::{address::AddressHashMap, Amount};
use models::{Address, Block, BlockId, OperationType, Slot};
use parking_lot::Mutex;
use std::collections::hash_map;
use std::fmt::Debug;
use std::sync::Arc;
use tracing::debug;
use wasmer::{imports, Function, ImportObject, Instance, Module, Store, WasmerEnv};

/// Example API available to wasm code, aka "syscall".
fn foo(shared_env: &SharedExecutionContext, n: i32) -> i32 {
    n
}

/// API allowing a contract to call another.
fn call_address(shared_env: &SharedExecutionContext, addr: Address) {
    //if let Some(ledger_entry) = Arc::clone(&shared_env.0).lock().active_ledger.get(&addr) {}
    //TODO
}

pub struct ExecutionStep {
    pub slot: Slot,
    // TODO add pos_seed for RNG seeding
    // TODO add pos_draws to list the draws (block and endorsement creators) for that slot
    pub block: Option<(BlockId, Block)>, // None if miss
}

#[derive(Debug, Clone, Default)]
pub struct SCELedgerEntry {
    pub balance: Amount,
    pub module: Option<Module>,
    pub data: HHashMap<Hash, Vec<u8>>,
}

#[derive(WasmerEnv, Clone)]
/// Stateful context, providing an execution context to host functions("syscalls").
pub struct ExecutionContext {
    pub final_ledger: AddressHashMap<SCELedgerEntry>,
    pub active_ledger: AddressHashMap<SCELedgerEntry>,
}

#[derive(WasmerEnv, Clone)]
pub struct SharedExecutionContext(pub Arc<Mutex<ExecutionContext>>);

pub struct VM {
    cfg: ExecutionConfig,
    imports: ImportObject,
    store: Store,
    shared_execution_context: SharedExecutionContext,
}

impl VM {
    pub fn new(cfg: ExecutionConfig) -> VM {
        let store = Store::default();
        let shared_execution_context =
            SharedExecutionContext(Arc::new(Mutex::new(ExecutionContext {
                final_ledger: Default::default(),  // TODO bootstrap
                active_ledger: Default::default(), // TODO bootstrap
            })));
        let imports = imports! {
            "env" => {
                "foo" => Function::new_native_with_env(&store, shared_execution_context.clone(), foo),
            },
        };
        VM {
            cfg,
            imports,
            store,
            shared_execution_context,
        }
    }

    /// runs an SCE-final execution step
    /// # Parameters
    ///   * step: execution step to run
    pub fn run_final_step(&mut self, step: ExecutionStep) {
        // TODO metering (ex: per-block gas limit)

        // run implicit and async calls
        //TODO

        // run explicit calls within the block
        if let Some((block_id, block)) = step.block {
            // get block creator addr
            // TODO remove unwrap
            let block_creator_addr =
                Address::from_public_key(&block.header.content.creator).unwrap();

            let mut remaining_gas = self.cfg.max_gas_per_block;

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

                    // get sender address
                    // TODO remove unwrap
                    let sender_addr =
                        Address::from_public_key(&operation.content.sender_public_key).unwrap();

                    // credit the sender with "coins"
                    // we do it early because otherwise the block creator can make it cancellable (out of gas)
                    // but the amount was already spend on the CSS side.
                    // This will be corrected with the single-ledger version of the smart contract system.
                    {
                        let mut context_guard = self.shared_execution_context.0.lock();
                        let ledger_entry = (*context_guard)
                            .final_ledger
                            .entry(sender_addr)
                            .or_insert_with(|| Default::default());
                        // TODO remove unwrap()
                        ledger_entry.balance = ledger_entry.balance.checked_add(coins).unwrap();
                    }

                    // check remaining per-block gas
                    match remaining_gas.checked_sub(max_gas) {
                        Some(remaining) => remaining_gas = remaining,
                        None => continue,
                    }

                    // credit the block creator with max_gas*gas_price
                    // TODO consider dispatching with edorsers/endorsed as well
                    // TODO remove unwrap()
                    {
                        let to_credit = gas_price.checked_mul_u64(max_gas).unwrap();
                        let mut context_guard = self.shared_execution_context.0.lock();
                        let ledger_entry = (*context_guard)
                            .final_ledger
                            .entry(block_creator_addr)
                            .or_insert_with(|| Default::default());
                        // TODO remove unwrap()
                        ledger_entry.balance = ledger_entry.balance.checked_add(to_credit).unwrap();
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

                // a module was successfully parsed and balances are ready: run the module
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

    /// runs an SCE-active execution step
    /// # Parameters
    ///   * step: execution step to run
    pub fn run_active_step(&mut self, step: ExecutionStep) {
        // TODO
    }

    /// resets the VM to its latest final state
    pub fn reset_to_final(&mut self) {
        let mut context_guard = self.shared_execution_context.0.lock();
        // reset ledger to its latest final state
        (*context_guard).active_ledger = (*context_guard).final_ledger.clone();
    }
}
