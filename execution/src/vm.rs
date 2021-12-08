use crypto::hash::Hash;
use models::address::AddressHashMap;
use models::{Address, Amount, Block, BlockId, OperationType, Slot};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::debug;
use wasmtime::*;

use crate::sce_ledger::{SCELedger, SCELedgerChanges, SCELedgerStep};
use crate::ExecutionConfig;

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
    pub call_stack: VecDeque<Address>,
}

#[derive(Clone)]
/// A map of modules, a shared u32, and a list of imports.
pub struct SharedExecutionContext(pub Arc<Mutex<(AddressHashMap<Module>, u32, Vec<Extern>)>>);

pub struct VM {
    cfg: ExecutionConfig,
    final_ledger: Arc<Mutex<SCELedger>>,
    step_history: VecDeque<(Slot, Option<BlockId>, SCELedgerChanges)>,
    current_execution_context: SharedExecutionContext,
}

impl VM {
    pub fn new(cfg: ExecutionConfig) -> VM {
        // Execute a module that will:
        // 1. Call into another module.
        // 2. The second module will, when called, mutate some shared state.
        let engine = Engine::default();
        let final_ledger = Arc::new(Mutex::new(
            SCELedger::default(), // TODO bootstrap
        ));

        let current_execution_context = {
            // Add a module for the address "hello world".
            // The module will call `host_set` to mutate the shared u32.
            let mut modules: AddressHashMap<Module> = Default::default();
            let wat = r#"
            (module
                (import "host" "set" (func $host_set (param i32)))
                (import "host" "call" (func $host_call (param i32 i32)))
                (func (export "main")
                    i32.const 3
                    call $host_set)
            )
        "#;
            let module = Module::new(&engine, wat).unwrap();
            let hash = Hash::hash(&"Hello, world!".as_bytes());
            let serialized = hash.into_bytes();
            let addr = Address::from_bytes(&serialized).unwrap();
            modules.insert(addr, module);
            SharedExecutionContext(Arc::new(Mutex::new((modules, 0, Default::default()))))
        };
        let mut store = Store::new(&engine, current_execution_context.clone());

        // The two APIs.
        let host_call = Func::wrap(
            &mut store,
            move |mut caller: Caller<'_, SharedExecutionContext>, ptr: i32, len: i32| {
                println!("Start host call");
                let mem = match caller.get_export("memory") {
                    Some(Extern::Memory(mem)) => mem,
                    _ => return Err(Trap::new("failed to find host memory")),
                };
                let data = mem
                    .data(&caller)
                    .get(ptr as u32 as usize..)
                    .and_then(|arr| arr.get(..len as u32 as usize));

                // Read the "address" from the memory of the module.
                let string = match data {
                    Some(data) => match std::str::from_utf8(data) {
                        Ok(s) => s,
                        Err(_) => return Err(Trap::new("invalid utf-8")),
                    },
                    None => return Err(Trap::new("pointer/length out of bounds")),
                };

                // Get the module for the address.
                let (module, imports) = {
                    let context = caller.data().0.lock().unwrap();
                    let hash = Hash::hash(&string.as_bytes());
                    let serialized = hash.into_bytes();
                    let addr = Address::from_bytes(&serialized).unwrap();
                    let module = context.0.get(&addr).unwrap().clone();
                    let imports = context.2.clone();
                    (module, imports)
                };

                // Instantiate, and call, the module.
                let instance = Instance::new(&mut caller, &module, &imports).unwrap();
                let foo = instance
                    .get_typed_func::<(), (), _>(&mut caller, "main")
                    .unwrap();
                foo.call(&mut caller, ()).unwrap();
                Ok(())
            },
        );
        let host_set = Func::wrap(
            &mut store,
            |mut caller: Caller<'_, SharedExecutionContext>, to_set: i32| {
                println!("Start host set");
                let mut context = caller.data().0.lock().unwrap();
                println!("Setting.");
                // Mutate the shared u32.
                context.1 = to_set as u32;
                Ok(())
            },
        );

        current_execution_context.0.lock().unwrap().2 = vec![host_set.into(), host_call.into()];

        // The module that will call into `host_call`, to execute another module for the "hello world" address.
        let wat = r#"
        (module
            (import "host" "set" (func $host_set (param i32)))
            (import "host" "call" (func $host_call (param i32 i32)))
            (func (export "main")
                i32.const 4   ;; ptr
                i32.const 13  ;; len
                call $host_call)
            (memory (export "memory") 1)
            (data (i32.const 4) "Hello, world!"))
    "#;
        let module = Module::new(&engine, wat).unwrap();

        // Instantiation of a module requires specifying its imports and then
        // afterwards we can fetch exports by name, as well as asserting the
        // type signature of the function with `get_typed_func`.
        let imports = current_execution_context.0.lock().unwrap().2.clone();
        let instance = Instance::new(&mut store, &module, &imports).unwrap();

        let foo = instance
            .get_typed_func::<(), (), _>(&mut store, "main")
            .unwrap();
        foo.call(&mut store, ()).unwrap();

        // Check that the value has been set by the second contract.
        assert_eq!(current_execution_context.0.lock().unwrap().1, 3);

        println!("OK!");

        VM {
            cfg,
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
                } else {
                    // not an ExecuteSC operations
                    continue;
                };
            }
        } else {
            // miss
            opt_block_id = None;
        }
    }

    /// resets the VM to its latest final state
    pub fn reset_to_final(&mut self) {
        self.step_history.clear();
    }
}
