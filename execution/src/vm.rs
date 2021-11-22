use models::address::AddressHashMap;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use wasmer::{imports, Function, ImportObject, Module, Store};

/// Example API available to wasm code, aka "syscall".
fn foo(n: i32) -> i32 {
    n
}

/// API allowing a contract to call another.
fn call_address(addr: ()) {}

pub struct VM {
    imports: ImportObject,
    store: Arc<Store>,
    ledger: Arc<Mutex<AddressHashMap<Module>>>,
}

impl VM {
    pub fn new(store: Arc<Store>, ledger: Arc<Mutex<AddressHashMap<Module>>>) -> VM {
        let imports = imports! {
            "env" => {
                "foo" => Function::new_native(&store, foo)
            },
        };
        VM {
            imports,
            store,
            ledger,
        }
    }

    pub fn run(&self, to_run: &[Module]) {
        for module in to_run {}
    }
}
