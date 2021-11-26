use models::address::AddressHashMap;
use models::Address;
use parking_lot::Mutex;
use std::sync::Arc;
use wasmer::{imports, Function, ImportObject, Instance, Module, Store, WasmerEnv};

/// Example API available to wasm code, aka "syscall".
fn foo(env: &ExecutionContext, n: i32) -> i32 {
    n
}

/// API allowing a contract to call another.
fn call_address(env: &ExecutionContext, addr: Address) {
    if let Some(module) = env.ledger.lock().get(&addr) {}
}

#[derive(WasmerEnv, Clone)]
/// Stateful context, providing an execution context to host functions("syscalls").
pub struct ExecutionContext {
    ledger: Arc<Mutex<AddressHashMap<Module>>>,
}

pub struct VM {
    imports: ImportObject,
    store: Arc<Store>,
    ledger: Arc<Mutex<AddressHashMap<Module>>>,
}

impl VM {
    pub fn new(store: Arc<Store>, ledger: Arc<Mutex<AddressHashMap<Module>>>) -> VM {
        let imports = imports! {
            "env" => {
                "foo" => Function::new_native_with_env(&store, ExecutionContext {ledger: ledger.clone()}, foo),
            },
        };
        VM {
            imports,
            store,
            ledger,
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
