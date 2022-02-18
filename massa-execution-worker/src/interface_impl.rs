/// Implementation of the interface used in the execution external library
///
use crate::context::ExecutionContext;
use anyhow::{bail, Result};
use massa_execution_exports::ExecutionConfig;
use massa_execution_exports::ExecutionStackElement;
use massa_hash::hash::Hash;
use massa_models::{
    output_event::{EventExecutionContext, SCOutputEvent, SCOutputEventId},
    timeslots::get_block_slot_timestamp,
    AMOUNT_ZERO,
};
use massa_sc_runtime::{Interface, InterfaceClone};
use rand::Rng;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tracing::debug;

macro_rules! context_guard {
    ($self:ident) => {
        $self
            .context
            .lock()
            .expect("failed to acquire lock on execution context")
    };
}

#[derive(Clone)]
pub(crate) struct InterfaceImpl {
    config: ExecutionConfig,
    context: Arc<Mutex<ExecutionContext>>,
}

impl InterfaceImpl {
    pub fn new(config: ExecutionConfig, context: Arc<Mutex<ExecutionContext>>) -> InterfaceImpl {
        InterfaceImpl { config, context }
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

    fn init_call(&self, address: &str, raw_coins: u64) -> Result<Vec<u8>> {
        // get target
        let to_address = massa_models::Address::from_str(address)?;

        // write-lock context
        let mut context = context_guard!(self);

        // get bytecode
        let bytecode = match context.get_bytecode(&to_address) {
            Some(bytecode) => bytecode,
            None => bail!("bytecode not found for address {}", to_address),
        };

        // get caller
        let from_address = match context.stack.last() {
            Some(addr) => addr.address,
            _ => bail!("failed to read call stack current address"),
        };

        // transfer coins
        let coins = massa_models::Amount::from_raw(raw_coins);
        if let Err(err) =
            context.transfer_parallel_coins(Some(from_address), Some(to_address), coins, true)
        {
            bail!(
                "error transferring {} parallel coins from {} to {}: {}",
                coins,
                from_address,
                to_address,
                err
            );
        }

        // prepare context
        context.stack.push(ExecutionStackElement {
            address: to_address,
            coins,
            owned_addresses: vec![to_address],
        });

        Ok(bytecode)
    }

    fn finish_call(&self) -> Result<()> {
        let mut context = context_guard!(self);

        if context.stack.pop().is_none() {
            bail!("call stack out of bounds")
        }

        Ok(())
    }

    /// Returns zero as a default if address not found.
    fn get_balance(&self) -> Result<u64> {
        let context = context_guard!(self);
        let address = context.get_current_address()?;
        Ok(context
            .get_parallel_balance(&address)
            .unwrap_or(AMOUNT_ZERO)
            .to_raw())
    }

    /// Returns zero as a default if address not found.
    fn get_balance_for(&self, address: &str) -> Result<u64> {
        let address = massa_models::Address::from_str(address)?;
        Ok(context_guard!(self)
            .get_parallel_balance(&address)
            .unwrap_or(AMOUNT_ZERO)
            .to_raw())
    }

    /// Requires a new address that contains the sent bytecode.
    ///
    /// Generate a new address with a concatenation of the block_id hash, the
    /// operation index in the block and the index of address owned in context.
    ///
    /// Insert in the ledger the given bytecode in the generated address
    fn create_module(&self, module: &[u8]) -> Result<String> {
        match context_guard!(self).create_new_sc_address(module.to_vec()) {
            Ok(addr) => Ok(addr.to_bs58_check()),
            Err(err) => bail!("couldn't create new SC address: {}", err),
        }
    }

    /// Requires the data at the address
    fn raw_get_data_for(&self, address: &str, key: &str) -> Result<Vec<u8>> {
        let addr = &massa_models::Address::from_bs58_check(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        match context.get_data_entry(addr, &key) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Requires to replace the data in the current address
    ///
    /// Note:
    /// The execution lib will allways use the current context address for the update
    fn raw_set_data_for(&self, address: &str, key: &str, value: &[u8]) -> Result<()> {
        let addr = massa_models::Address::from_str(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let mut context = context_guard!(self);
        context.set_data_entry(&addr, key, value.to_vec(), true)?;
        Ok(())
    }

    fn has_data_for(&self, address: &str, key: &str) -> Result<bool> {
        let addr = massa_models::Address::from_str(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        Ok(context.has_data_entry(&addr, &key))
    }

    fn raw_get_data(&self, key: &str) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let addr = context.get_current_address()?;
        match context.get_data_entry(&addr, &key) {
            Some(data) => Ok(data),
            _ => bail!("data entry not found"),
        }
    }

    fn raw_set_data(&self, key: &str, value: &[u8]) -> Result<()> {
        let mut context = context_guard!(self);
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let addr = context.get_current_address()?;
        context.set_data_entry(&addr, key, value.to_vec(), true)?;
        Ok(())
    }

    fn has_data(&self, key: &str) -> Result<bool> {
        let context = context_guard!(self);
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let addr = context.get_current_address()?;
        Ok(context.has_data_entry(&addr, &key))
    }

    /// hash data
    fn hash(&self, data: &[u8]) -> Result<String> {
        Ok(massa_hash::hash::Hash::compute_from(data).to_bs58_check())
    }

    /// convert a pubkey to an address
    fn address_from_public_key(&self, public_key: &str) -> Result<String> {
        let public_key = massa_signature::PublicKey::from_bs58_check(public_key)?;
        let addr = massa_models::Address::from_public_key(&public_key);
        Ok(addr.to_bs58_check())
    }

    /// Verify signature
    fn signature_verify(&self, data: &[u8], signature: &str, public_key: &str) -> Result<bool> {
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

    /// Transfer parallel coins from the current address to a target address
    /// to_address: target address
    /// raw_amount: amount to transfer (in raw u64)
    fn transfer_coins(&self, to_address: &str, raw_amount: u64) -> Result<()> {
        let to_address = massa_models::Address::from_str(to_address)?;
        let amount = massa_models::Amount::from_raw(raw_amount);
        let mut context = context_guard!(self);
        let from_address = context.get_current_address()?;
        context.transfer_parallel_coins(Some(from_address), Some(to_address), amount, true)?;
        Ok(())
    }

    /// Transfer coins from the current address to a target address
    /// from_address: source address
    /// to_address: target address
    /// raw_amount: amount to transfer (in raw u64)
    fn transfer_coins_for(
        &self,
        from_address: &str,
        to_address: &str,
        raw_amount: u64,
    ) -> Result<()> {
        let from_address = massa_models::Address::from_str(from_address)?;
        let to_address = massa_models::Address::from_str(to_address)?;
        let amount = massa_models::Amount::from_raw(raw_amount);
        let mut context = context_guard!(self);
        context.transfer_parallel_coins(Some(from_address), Some(to_address), amount, true)?;
        Ok(())
    }

    /// Return the list of owned adresses of a given SC user
    fn get_owned_addresses(&self) -> Result<Vec<String>> {
        Ok(context_guard!(self)
            .get_current_owned_addresses()?
            .into_iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }

    /// Return the call stack (addresses)
    fn get_call_stack(&self) -> Result<Vec<String>> {
        Ok(context_guard!(self)
            .get_call_stack()
            .into_iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }

    /// Get the amount of coins that have been made available for use by the caller of the currently executing code.
    fn get_call_coins(&self) -> Result<u64> {
        Ok(context_guard!(self).get_current_call_coins()?.to_raw())
    }

    /// generate an execution event and stores it
    fn generate_event(&self, data: String) -> Result<()> {
        let mut execution_context = context_guard!(self);

        // prepare id computation
        // it is the hash of (slot, index_at_slot, readonly)
        let mut to_hash: Vec<u8> = execution_context.slot.to_bytes_key().to_vec();
        to_hash.append(&mut execution_context.created_event_index.to_be_bytes().to_vec());
        to_hash.push(!execution_context.read_only as u8);

        let context = EventExecutionContext {
            slot: execution_context.slot,
            block: execution_context.opt_block_id,
            call_stack: execution_context.stack.iter().map(|e| e.address).collect(),
            read_only: execution_context.read_only,
            index_in_slot: execution_context.created_event_index,
            origin_operation_id: execution_context.origin_operation_id,
        };
        let id = SCOutputEventId(Hash::compute_from(&to_hash));
        let event = SCOutputEvent { id, context, data };
        execution_context.created_event_index += 1;
        execution_context.events.insert(id, event);
        Ok(())
    }

    /// Returns the current time (millisecond unix timestamp)
    fn get_time(&self) -> Result<u64> {
        let slot = context_guard!(self).slot;
        let ts = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            slot,
        )?;
        Ok(ts.to_millis())
    }

    /// Returns a random number (unsafe: can be predicted and manipulated)
    fn unsafe_random(&self) -> Result<i64> {
        let distr = rand::distributions::Uniform::new_inclusive(i64::MIN, i64::MAX);
        Ok(context_guard!(self).unsafe_rng.sample(distr))
    }
}
