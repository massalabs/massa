/// Implementation of the interface used in the execution external library
///
use crate::types::ExecutionContext;
use anyhow::{bail, Result};
use massa_hash::hash::Hash;
use massa_models::{
    output_event::{EventExecutionContext, SCOutputEvent, SCOutputEventId},
    timeslots::get_block_slot_timestamp,
    AMOUNT_ZERO,
};
use massa_sc_runtime::{Interface, InterfaceClone};
use massa_time::MassaTime;
use rand::Rng;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tracing::debug;

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
    thread_count: u8,
    t0: MassaTime,
    genesis_timestamp: MassaTime,
}

impl InterfaceImpl {
    pub fn new(
        context: Arc<Mutex<ExecutionContext>>,
        thread_count: u8,
        t0: MassaTime,
        genesis_timestamp: MassaTime,
    ) -> InterfaceImpl {
        InterfaceImpl {
            context,
            thread_count,
            t0,
            genesis_timestamp,
        }
    }
}

impl InterfaceClone for InterfaceImpl {
    fn clone_box(&self) -> Box<dyn Interface> {
        Box::new(self.clone())
    }
}

impl Interface for InterfaceImpl {
    fn print(&self, message: &str) -> Result<()> {
        debug!("SC print: {}", message);
        Ok(())
    }

    fn init_call(&self, address: &String, raw_coins: u64) -> Result<Vec<u8>> {
        // get target
        let to_address = massa_models::Address::from_str(address)?;

        // write-lock context
        let mut context = context_guard!(self);

        // get bytecode
        let bytecode = match context.ledger_step.get_module(&to_address) {
            Some(bytecode) => bytecode,
            None => bail!("Error bytecode not found"),
        };

        // get caller
        let from_address = match context.call_stack.back() {
            Some(addr) => *addr,
            _ => bail!("Failed to read call stack current address"),
        };

        // transfer coins
        let coins = massa_models::Amount::from_raw(raw_coins);
        // debit
        context
            .ledger_step
            .set_balance_delta(from_address, coins, false)?;
        // credit
        if let Err(err) = context
            .ledger_step
            .set_balance_delta(to_address, coins, true)
        {
            // cancel debit
            context
                .ledger_step
                .set_balance_delta(from_address, coins, true)
                .expect("credit failed after same-amount debit succeeded");
            bail!("Error crediting destination balance: {}", err);
        }

        // prepare context
        context.call_stack.push_back(to_address);
        context.coins_stack.push(coins);
        context.owned_addresses_stack.push(vec![to_address]);

        Ok(bytecode)
    }

    fn finish_call(&self) -> Result<()> {
        let mut context = context_guard!(self);

        if context.call_stack.pop_back().is_none() {
            bail!("call stack out of bounds")
        }

        if context.coins_stack.pop().is_none() {
            bail!("coins stack out of bounds")
        }

        if context.owned_addresses_stack.pop().is_none() {
            bail!("owned addresses stack out of bounds")
        }

        Ok(())
    }

    /// Returns zero as a default if address not found.
    fn get_balance(&self) -> Result<u64> {
        let context = context_guard!(self);
        let address = match context.call_stack.back() {
            Some(addr) => addr,
            _ => bail!("Failed to read call stack current address"),
        };
        Ok(context.ledger_step.get_balance(address).to_raw())
    }

    /// Returns zero as a default if address not found.
    fn get_balance_for(&self, address: &String) -> Result<u64> {
        let address = massa_models::Address::from_str(address)?;
        Ok(context_guard!(self)
            .ledger_step
            .get_balance(&address)
            .to_raw())
    }

    /// Requires a new address that contains the sent bytecode.
    ///
    /// Generate a new address with a concatenation of the block_id hash, the
    /// operation index in the block and the index of address owned in context.
    ///
    /// Insert in the ledger the given bytecode in the generated address
    fn create_module(
        &self,
        module: &massa_sc_runtime::Bytecode,
    ) -> Result<massa_sc_runtime::Address> {
        let mut context = context_guard!(self);
        let (slot, created_addr_index) = (context.slot, context.created_addr_index);
        let mut data: Vec<u8> = slot.to_bytes_key().to_vec();
        data.append(&mut created_addr_index.to_be_bytes().to_vec());
        if context.read_only {
            data.push(0u8);
        } else {
            data.push(1u8);
        }
        let address = massa_models::Address(massa_hash::hash::Hash::compute_from(&data));
        let res = address.to_bs58_check();
        context
            .ledger_step
            .set_module(address, Some(module.clone()));
        match context.owned_addresses_stack.last_mut() {
            Some(v) => {
                v.push(address);
            }
            None => bail!("owned addresses not found in stack"),
        };
        context.created_addr_index += 1;
        Ok(res)
    }

    /// Requires the data at the address
    fn get_data_for(
        &self,
        address: &massa_sc_runtime::Address,
        key: &String,
    ) -> Result<massa_sc_runtime::Bytecode> {
        let addr = &massa_models::Address::from_bs58_check(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        match context.ledger_step.get_data_entry(addr, &key) {
            Some(value) => Ok(value),
            _ => bail!("Data entry not found"),
        }
    }

    /// Requires to replace the data in the current address
    ///
    /// Note:
    /// The execution lib will allways use the current context address for the update
    fn set_data_for(
        &self,
        address: &massa_sc_runtime::Address,
        key: &String,
        value: &[u8],
    ) -> Result<()> {
        let addr = massa_models::Address::from_str(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let mut context = context_guard!(self);
        let is_allowed = context
            .owned_addresses_stack
            .last()
            .map_or(false, |v| v.contains(&addr));
        if !is_allowed {
            bail!("You don't have the write access to this entry")
        }
        context
            .ledger_step
            .set_data_entry(addr, key, value.to_vec());
        Ok(())
    }

    fn has_data_for(&self, address: &massa_sc_runtime::Address, key: &str) -> Result<bool> {
        let context = context_guard!(self);
        let addr = massa_models::Address::from_str(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        Ok(context.ledger_step.has_data_entry(&addr, &key))
    }

    fn get_data(&self, key: &str) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let addr = match context.call_stack.back() {
            Some(addr) => addr,
            _ => bail!("Failed to read call stack current address"),
        };
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        match context.ledger_step.get_data_entry(addr, &key) {
            Some(bytecode) => Ok(bytecode),
            _ => bail!("Data entry not found"),
        }
    }

    fn set_data(&self, key: &str, value: &[u8]) -> Result<()> {
        let mut context = context_guard!(self);
        let addr = match context.call_stack.back() {
            Some(addr) => *addr,
            _ => bail!("Failed to read call stack current address"),
        };
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        context
            .ledger_step
            .set_data_entry(addr, key, value.to_vec());
        Ok(())
    }

    fn has_data(&self, key: &str) -> Result<bool> {
        let context = context_guard!(self);
        let addr = match context.call_stack.back() {
            Some(addr) => addr,
            _ => bail!("Failed to read call stack current address"),
        };
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        Ok(context.ledger_step.has_data_entry(addr, &key))
    }

    /// hash data
    fn hash(&self, data: &[u8]) -> Result<massa_sc_runtime::MassaHash> {
        Ok(massa_hash::hash::Hash::compute_from(data).to_bs58_check())
    }

    /// convert a pubkey to an address
    fn address_from_public_key(
        &self,
        public_key: &massa_sc_runtime::PublicKey,
    ) -> Result<massa_sc_runtime::Address> {
        let public_key = massa_signature::PublicKey::from_bs58_check(public_key)?;
        let addr = massa_models::Address::from_public_key(&public_key);
        Ok(addr.to_bs58_check())
    }

    /// Verify signature
    fn signature_verify(
        &self,
        data: &[u8],
        signature: &massa_sc_runtime::Signature,
        public_key: &massa_sc_runtime::PublicKey,
    ) -> Result<bool> {
        let signature = match massa_signature::Signature::from_bs58_check(signature) {
            Ok(sig) => sig,
            Err(_) => return Ok(false),
        };
        let public_key = match massa_signature::PublicKey::from_bs58_check(public_key) {
            Ok(pubk) => pubk,
            Err(_) => return Ok(false),
        };
        let h = massa_hash::hash::Hash::compute_from(data);
        Ok(massa_signature::verify_signature(&h, &signature, &public_key).is_ok())
    }

    /// Transfer coins from the current address to a target address
    /// to_address: target address
    /// raw_amount: amount to transfer (in raw u64)
    fn transfer_coins(&self, to_address: &String, raw_amount: u64) -> Result<()> {
        let to_address = massa_models::Address::from_str(to_address)?;
        let mut context = context_guard!(self);
        let from_address = match context.call_stack.back() {
            Some(addr) => *addr,
            _ => bail!("Failed to read call stack current address"),
        };
        let amount = massa_models::Amount::from_raw(raw_amount);
        // debit
        context
            .ledger_step
            .set_balance_delta(from_address, amount, false)?;
        // credit
        if let Err(err) = context
            .ledger_step
            .set_balance_delta(to_address, amount, true)
        {
            // cancel debit
            context
                .ledger_step
                .set_balance_delta(from_address, amount, true)
                .expect("credit failed after same-amount debit succeeded");
            bail!("Error crediting destination balance: {}", err);
        }
        Ok(())
    }

    /// Transfer coins from the current address to a target address
    /// from_address: source address
    /// to_address: target address
    /// raw_amount: amount to transfer (in raw u64)
    fn transfer_coins_for(
        &self,
        from_address: &String,
        to_address: &String,
        raw_amount: u64,
    ) -> Result<()> {
        let from_address = massa_models::Address::from_str(from_address)?;
        let to_address = massa_models::Address::from_str(to_address)?;
        let mut context = context_guard!(self);
        let is_allowed = context
            .owned_addresses_stack
            .last()
            .map_or(false, |v| v.contains(&from_address));
        if !is_allowed {
            bail!("You don't have the spending access to this entry")
        }
        let amount = massa_models::Amount::from_raw(raw_amount);
        // debit
        context
            .ledger_step
            .set_balance_delta(from_address, amount, false)?;
        // credit
        if let Err(err) = context
            .ledger_step
            .set_balance_delta(to_address, amount, true)
        {
            // cancel debit
            context
                .ledger_step
                .set_balance_delta(from_address, amount, true)
                .expect("credit failed after same-amount debit succeeded");
            bail!("Error crediting destination balance: {}", err);
        }
        Ok(())
    }

    /// Return the list of owned adresses of a given SC user
    fn get_owned_addresses(&self) -> Result<Vec<massa_sc_runtime::Address>> {
        match context_guard!(self).owned_addresses_stack.last() {
            Some(v) => Ok(v.iter().map(|addr| addr.to_bs58_check()).collect()),
            None => bail!("owned address stack out of bounds"),
        }
    }

    fn get_call_stack(&self) -> Result<Vec<massa_sc_runtime::Address>> {
        Ok(context_guard!(self)
            .call_stack
            .iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }

    /// Get the amount of coins that have been made available for use by the caller of the currently executing code.
    fn get_call_coins(&self) -> Result<u64> {
        Ok(context_guard!(self)
            .coins_stack
            .last()
            .unwrap_or(&AMOUNT_ZERO)
            .to_raw())
    }

    fn generate_event(&self, data: String) -> Result<()> {
        let mut execution_context = context_guard!(self);

        let mut to_hash: Vec<u8> = execution_context.slot.to_bytes_key().to_vec();
        to_hash.append(&mut execution_context.created_event_index.to_be_bytes().to_vec());
        to_hash.push(!execution_context.read_only as u8);

        let context = EventExecutionContext {
            slot: execution_context.slot,
            block: execution_context.opt_block_id,
            call_stack: execution_context.call_stack.clone(),
        };
        let id = Hash::compute_from(&to_hash);
        let event = SCOutputEvent {
            id: SCOutputEventId(id),
            context,
            data,
            read_only: execution_context.read_only,
        };
        execution_context.created_event_index += 1;
        execution_context.events.insert(id, event);
        Ok(())
    }

    /// Returns the current time (millisecond unix timestamp)
    fn get_time(&self) -> Result<u64> {
        let slot = context_guard!(self).slot;
        let ts =
            get_block_slot_timestamp(self.thread_count, self.t0, self.genesis_timestamp, slot)?;
        Ok(ts.to_millis())
    }

    /// Returns a random number (unsafe: can be predicted and manipulated)
    fn unsafe_random(&self) -> Result<i64> {
        let distr = rand::distributions::Uniform::new_inclusive(i64::MIN, i64::MAX);
        Ok(context_guard!(self).unsafe_rng.sample(distr))
    }
}
