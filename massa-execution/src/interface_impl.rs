/// Implementation of the interface used in the execution external library
///
use std::str::FromStr;

use crate::types::ExecutionContext;
use anyhow::{bail, Result};
use assembly_simulator::{Interface, InterfaceClone};
use massa_models::Address;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct InterfaceImpl {
    context: Arc<Mutex<ExecutionContext>>,
}

impl InterfaceImpl {
    pub fn new(context: Arc<Mutex<ExecutionContext>>) -> InterfaceImpl {
        InterfaceImpl { context }
    }
}

impl InterfaceClone for InterfaceImpl {
    fn clone_box(&self) -> Box<dyn Interface> {
        Box::new(self.clone())
    }
}

impl Interface for InterfaceImpl {
    fn get_module(&self, address: &String) -> Result<Vec<u8>> {
        let bytecode = {
            let context = self
                .context
                .lock()
                .expect("Failed to acquire lock on context.");
            context.ledger_step.get_module(
                &Address::from_str(address).expect("Failed to convert sting to address."),
            )
        };
        match bytecode {
            Some(bytecode) => Ok(bytecode),
            _ => bail!("Error bytecode not found"),
        }
    }
}
