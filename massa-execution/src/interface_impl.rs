/// Implementation of the interface used in the execution external library
///
use std::str::FromStr;

use crate::types::ExecutionContext;
use anyhow::{bail, Result};
use assembly_simulator::{Bytecode, Interface, InterfaceClone};
use massa_models::Address;
use std::sync::{Arc, Mutex};

macro_rules! context_guard {
    ($self:ident) => {
        $self
            .context
            .lock()
            .expect("Failed to acquire lock on context.")
    };
}

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
        let address = Address::from_str(address)?;
        let bytecode = { context_guard!(self).ledger_step.get_module(&address) };
        match bytecode {
            Some(bytecode) => {
                context_guard!(self).call_stack.push_back(address);
                Ok(bytecode)
            }
            _ => bail!("Error bytecode not found"),
        }
    }

    fn exit_success(&self) -> Result<()> {
        match context_guard!(self).call_stack.pop_back() {
            Some(_) => Ok(()),
            _ => bail!("Call stack Out of bound"),
        }
    }

    /// Requires a new address that contains the sent bytecode.
    ///
    /// Generate a new address with a concatenation of the block_id hash and the index of address owned in context.
    ///
    /// Insert in the ledger the given bytecode in the generated address
    fn create_module(&self, module: &Bytecode) -> Result<assembly_simulator::Address> {
        let (block_id, index) = {
            let context = context_guard!(self);
            (context.opt_block_id, context.owned_addresses.len())
        };
        let block_id = match block_id {
            Some(block_id) => block_id,
            _ => bail!("Failed to read current context"),
        };
        let mut data = block_id.to_bytes().to_vec();
        data.append(&mut index.to_be_bytes().to_vec());
        let address = Address(massa_hash::hash::Hash::from(&data));
        let res = address.to_bs58_check();
        let mut context = context_guard!(self);
        context.ledger_step.set_module(address, module.clone());
        context.owned_addresses.push_back(address);
        Ok(res)
    }

    /// Requires the data at the address
    fn get_data_for(&self, address: &assembly_simulator::Address, key: &str) -> Result<Bytecode> {
        let addr = &Address::from_bs58_check(address)?;
        // @damip is it ok to get a hash like that?
        let key = massa_hash::hash::Hash::from_bs58_check(key)?;
        let context = context_guard!(self);
        match context.ledger_step.get_data_entry(addr, &key) {
            Some(bytecode) => Ok(bytecode),
            _ => bail!("Data entry not found"),
        }
    }

    /// Requires to replace the data in the current address
    ///
    /// Note:
    /// The execution lib will allways use the current context address for the update
    fn set_data_for(
        &self,
        address: &assembly_simulator::Address,
        key: &str,
        value: &Bytecode,
    ) -> Result<()> {
        let addr = Address::from_str(address)?;
        let key = massa_hash::hash::Hash::from_bs58_check(key)?;
        let mut context = context_guard!(self);
        if context.owned_addresses.contains(&addr) {
            context.ledger_step.set_data_entry(addr, key, value.clone());
        }
        bail!("You don't have the write access to this entry")
    }

    fn get_data(&self, key: &str) -> Result<Bytecode> {
        let context = context_guard!(self);
        let addr = match context.call_stack.front() {
            Some(addr) => addr,
            _ => bail!("Failed to read call stack current address"),
        };
        let key = massa_hash::hash::Hash::from_bs58_check(key)?;
        match context.ledger_step.get_data_entry(addr, &key) {
            Some(bytecode) => Ok(bytecode),
            _ => bail!("Data entry not found"),
        }
    }

    fn set_data(&self, key: &str, value: &Bytecode) -> Result<()> {
        let mut context = context_guard!(self);
        let addr = match context.call_stack.front() {
            Some(addr) => *addr,
            _ => bail!("Failed to read call stack current address"),
        };
        let key = massa_hash::hash::Hash::from_bs58_check(key)?;
        context.ledger_step.set_data_entry(addr, key, value.clone());
        Ok(())
    }

    /// Return the list of owned adresses of a given SC user
    fn get_owned_addresses(&self) -> Result<Vec<assembly_simulator::Address>> {
        Ok(context_guard!(self)
            .owned_addresses
            .iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }

    fn get_call_stack(&self) -> Result<Vec<assembly_simulator::Address>> {
        Ok(context_guard!(self)
            .call_stack
            .iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }
}
