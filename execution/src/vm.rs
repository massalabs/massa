use crypto::hash::Hash;
use models::hhasher::HHashMap;
use models::{Address, Block, BlockId, Slot};
use models::{address::AddressHashMap, Amount};
use parking_lot::Mutex;
use std::sync::Arc;
use wasmer::{imports, Function, ImportObject, Instance, Module, Store, WasmerEnv};

use crate::ExecutionConfig;

/// Example API available to wasm code, aka "syscall".
fn foo(shared_env: &SharedExecutionContext, n: i32) -> i32 {
    n
}

/// API allowing a contract to call another.
fn call_address(shared_env: &SharedExecutionContext, addr: Address) {
    if let Some(module) = Arc::clone(&shared_env.0).lock().active_ledger.get(&addr) {}
}

/// execution request
pub enum ExecutionRequest {
    RunFinalMiss(Slot), // Runs a final miss. TODO add the address drawn for this slot
    RunFinalBlock((BlockId, Block)), // Runs a final block
    RunActiveMiss(Slot), // Runs an active miss. TODO add the address drawn for this slot
    RunActiveBlock((BlockId, Block)), // Runs an active block
    ResetToFinalState,  // Resets to final state
    Stop,               // Stops the VM thread
}

#[derive(Debug, Clone)]
struct SCELedgerEntry {
    pub balance: Amount,
    pub module: Module,
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
    imports: ImportObject,
    store: Store,
    shared_execution_context: SharedExecutionContext,
}

impl VM {
    pub fn new() -> VM {
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
            imports,
            store,
            shared_execution_context,
        }
    }

    pub fn run(&self, to_run: &[Module]) {
        for module in to_run {
            self.call_module(module)
        }
    }

    fn call_module(&self, module: &Module) {
        let instance = Instance::new(&module, &self.imports).unwrap();
        let program = instance
            .exports
            .get_function("main")
            .unwrap()
            .native::<(), ()>()
            .unwrap();
        program.call();
    }
}
