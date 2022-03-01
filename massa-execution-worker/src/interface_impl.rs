// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Implementation of the interface between massa-execution-worker and massa-sc-runtime.
//! This allows the VM runtime to acceess the Massa execution context,
//! for example to interact with the ledger.
//! See the definition of Interface in the massa-sc-runtime crate for functional details.

use crate::context::ExecutionContext;
use anyhow::{bail, Result};
use massa_execution_exports::ExecutionConfig;
use massa_execution_exports::ExecutionStackElement;
use massa_hash::hash::Hash;
use massa_models::{
    output_event::{EventExecutionContext, SCOutputEvent, SCOutputEventId},
    timeslots::get_block_slot_timestamp,
};
use massa_sc_runtime::{Interface, InterfaceClone};
use parking_lot::Mutex;
use rand::Rng;
use std::str::FromStr;
use std::sync::Arc;
use tracing::debug;

/// helper for locking the context mutex
macro_rules! context_guard {
    ($self:ident) => {
        $self.context.lock()
    };
}

/// an implementation of the Interface trait (see massa-sc-runtime crate)
#[derive(Clone)]
pub(crate) struct InterfaceImpl {
    /// execution config
    config: ExecutionConfig,
    /// thread-safe shared access to the execution context (see context.rs)
    context: Arc<Mutex<ExecutionContext>>,
}

impl InterfaceImpl {
    /// creates a new InterfaceImpl
    ///
    /// # Arguments
    /// * config: execution config
    /// * context: thread-safe shared access to the current execution context (see context.rs)
    pub fn new(config: ExecutionConfig, context: Arc<Mutex<ExecutionContext>>) -> InterfaceImpl {
        InterfaceImpl { config, context }
    }
}

impl InterfaceClone for InterfaceImpl {
    /// allows cloning a boxed InterfaceImpl
    fn clone_box(&self) -> Box<dyn Interface> {
        Box::new(self.clone())
    }
}

/// Implementation of the Interface trait providing functions for massa-sc-runtime to call
/// in order to interact with the execution context during bytecode execution.
/// See the massa-sc-runtime crate for a functional description of the trait and its methods.
/// Note that massa-sc-runtime uses basic types (str for addresses, u64 for amounts...) for genericity.
impl Interface for InterfaceImpl {
    /// prints a message in the node logs at log level 3 (debug)
    fn print(&self, message: &str) -> Result<()> {
        debug!("SC print: {}", message);
        Ok(())
    }

    /// Initialize the call when bytecode calls a function from another bytecode
    /// This function transfers the coins passed as parameter,
    /// prepares the current execution context by pushing a new element on the top of the call stack,
    /// and returns the target bytecode from the ledger.
    ///
    /// # Arguments
    /// * address: string representation of the target address on which the bytecode will be called
    /// * raw_coins: raw representation (without decimal factor) of the amount of parallel coins to transfer from the caller address to the target address at the beginning of the call
    ///
    /// # Returns
    /// The target bytecode or an error
    fn init_call(&self, address: &str, raw_coins: u64) -> Result<Vec<u8>> {
        // get target address
        let to_address = massa_models::Address::from_str(address)?;

        // write-lock context
        let mut context = context_guard!(self);

        // get target bytecode
        let bytecode = match context.get_bytecode(&to_address) {
            Some(bytecode) => bytecode,
            None => bail!("bytecode not found for address {}", to_address),
        };

        // get caller address
        let from_address = match context.stack.last() {
            Some(addr) => addr.address,
            _ => bail!("failed to read call stack current address"),
        };

        // transfer coins from caller to target address
        let coins = massa_models::Amount::from_raw(raw_coins);
        if let Err(err) =
            context.transfer_parallel_coins(Some(from_address), Some(to_address), coins)
        {
            bail!(
                "error transferring {} parallel coins from {} to {}: {}",
                coins,
                from_address,
                to_address,
                err
            );
        }

        // push a new call stack element on top of the current call stack
        context.stack.push(ExecutionStackElement {
            address: to_address,
            coins,
            owned_addresses: vec![to_address],
        });

        // return the target bytecode
        Ok(bytecode)
    }

    /// Called to finish the call process after a bytecode calls a function from another one.
    /// This function just pops away the top element of the call stack.
    fn finish_call(&self) -> Result<()> {
        let mut context = context_guard!(self);

        if context.stack.pop().is_none() {
            bail!("call stack out of bounds")
        }

        Ok(())
    }

    /// Gets the parallel balance of the current address address (top of the stack).
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the parallel balance of the address,
    /// or zero if the address is not found in the ledger.
    fn get_balance(&self) -> Result<u64> {
        let context = context_guard!(self);
        let address = context.get_current_address()?;
        Ok(context
            .get_parallel_balance(&address)
            .unwrap_or_default()
            .to_raw())
    }

    /// Gets the parallel balance of arbitrary address passed as argument.
    ///
    /// # Arguments
    /// * address: string representation of the address for which to get the balance
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the parallel balance of the address,
    /// or zero if the address is not found in the ledger.
    fn get_balance_for(&self, address: &str) -> Result<u64> {
        let address = massa_models::Address::from_str(address)?;
        Ok(context_guard!(self)
            .get_parallel_balance(&address)
            .unwrap_or_default()
            .to_raw())
    }

    /// Creates a new ledger entry with the initial bytecode given as argument.
    /// A new unique address is generated for that entry and returned.
    ///
    /// # Arguments
    /// * bytecode: the bytecode to set for the newly created address
    ///
    /// # Returns
    /// The string representation of the newly created address
    fn create_module(&self, bytecode: &[u8]) -> Result<String> {
        match context_guard!(self).create_new_sc_address(bytecode.to_vec()) {
            Ok(addr) => Ok(addr.to_bs58_check()),
            Err(err) => bail!("couldn't create new SC address: {}", err),
        }
    }

    /// Gets a datastore value by key for a given address.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The datastore value matching the provided key, if found, otherwise an error.
    fn raw_get_data_for(&self, address: &str, key: &str) -> Result<Vec<u8>> {
        let addr = &massa_models::Address::from_bs58_check(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        match context.get_data_entry(addr, &key) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Sets a datastore entry for a given address
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to set
    /// * value: new value to set
    fn raw_set_data_for(&self, address: &str, key: &str, value: &[u8]) -> Result<()> {
        let addr = massa_models::Address::from_str(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let mut context = context_guard!(self);
        context.set_data_entry(&addr, key, value.to_vec())?;
        Ok(())
    }

    /// Checks if a datastore entry exists for a given address.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    fn has_data_for(&self, address: &str, key: &str) -> Result<bool> {
        let addr = massa_models::Address::from_str(address)?;
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        Ok(context.has_data_entry(&addr, &key))
    }

    /// Gets a datastore value by key for a the current address (top of the call stack).
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The datastore value matching the provided key, if found, otherwise an error.
    fn raw_get_data(&self, key: &str) -> Result<Vec<u8>> {
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        match context.get_data_entry(&addr, &key) {
            Some(data) => Ok(data),
            _ => bail!("data entry not found"),
        }
    }

    /// Sets a datastore entry for the current address (top of the call stack).
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to set
    /// * value: new value to set
    fn raw_set_data(&self, key: &str, value: &[u8]) -> Result<()> {
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let mut context = context_guard!(self);
        let addr = context.get_current_address()?;
        context.set_data_entry(&addr, key, value.to_vec())?;
        Ok(())
    }

    /// Checks if a datastore entry exists for the current address (top of the call stack).
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    fn has_data(&self, key: &str) -> Result<bool> {
        let key = massa_hash::hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        Ok(context.has_data_entry(&addr, &key))
    }

    /// Hashses arbitrary data
    ///
    /// # Arguments
    /// * data: data bytes to hash
    ///
    /// # Returns
    /// The string representation of the resulting hash
    fn hash(&self, data: &[u8]) -> Result<String> {
        Ok(massa_hash::hash::Hash::compute_from(data).to_bs58_check())
    }

    /// Converts a pubkey to an address
    ///
    /// # Arguments
    /// * public_key: string representation of the public key
    ///
    /// # Returns
    /// The string representation of the resulting address
    fn address_from_public_key(&self, public_key: &str) -> Result<String> {
        let public_key = massa_signature::PublicKey::from_bs58_check(public_key)?;
        let addr = massa_models::Address::from_public_key(&public_key);
        Ok(addr.to_bs58_check())
    }

    /// Verifies a signature
    ///
    /// # Arguments
    /// * data: the data bytes that were signed
    /// * signature: string representation of the signature
    /// * public key: string representation of the public key to check against
    ///
    /// # Returns
    /// true if the signature verification succeeded, false otherwise
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

    /// Transfer parallel coins from the current address (top of the call stack) towards a target address.
    ///
    /// # Arguments
    /// * to_address: string representation of the address to which the coins are sent
    /// * raw_amount: raw representation (no decimal factor) of the amount of coins to transfer
    fn transfer_coins(&self, to_address: &str, raw_amount: u64) -> Result<()> {
        let to_address = massa_models::Address::from_str(to_address)?;
        let amount = massa_models::Amount::from_raw(raw_amount);
        let mut context = context_guard!(self);
        let from_address = context.get_current_address()?;
        context.transfer_parallel_coins(Some(from_address), Some(to_address), amount)?;
        Ok(())
    }

    /// Transfer parallel coins from a given address towards a target address.
    ///
    /// # Arguments
    /// * from_address: string representation of the address that is sending the coins
    /// * to_address: string representation of the address to which the coins are sent
    /// * raw_amount: raw representation (no decimal factor) of the amount of coins to transfer
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
        context.transfer_parallel_coins(Some(from_address), Some(to_address), amount)?;
        Ok(())
    }

    /// Returns the list of owned adresses (top of the call stack).
    /// Those addresses are the ones the current execution context has write access to,
    /// typically it includes the current address itself,
    /// but also the ones that were created previously by the current call to allow initializing them.
    ///
    /// # Returns
    /// A vector with the string representation of each owned address.
    /// Note that the ordering of this vector is deterministic and conserved.
    fn get_owned_addresses(&self) -> Result<Vec<String>> {
        Ok(context_guard!(self)
            .get_current_owned_addresses()?
            .into_iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }

    /// Returns the addresses in the call stack, from the bottom to the top.
    ///
    /// # Returns
    /// A vector with the string representation of each call stack address.
    fn get_call_stack(&self) -> Result<Vec<String>> {
        Ok(context_guard!(self)
            .get_call_stack()
            .into_iter()
            .map(|addr| addr.to_bs58_check())
            .collect())
    }

    /// Gets the amount of coins that have been ransferred at the beginning of the call.
    /// See the init_call method.
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the amount of coins
    fn get_call_coins(&self) -> Result<u64> {
        Ok(context_guard!(self).get_current_call_coins()?.to_raw())
    }

    /// Emits an execution event to be stored.
    ///
    /// # Arguments:
    /// data: the string data that is the payload of the event
    fn generate_event(&self, data: String) -> Result<()> {
        let mut execution_context = context_guard!(self);

        // Generate a unique event ID
        // Initialize a seed from the current slot
        let mut to_hash: Vec<u8> = execution_context.slot.to_bytes_key().to_vec();
        // Append the index of the emitted event during the current slot
        to_hash.append(&mut execution_context.created_event_index.to_be_bytes().to_vec());
        // Append 0u8 if the context is readonly, 1u8 otherwise
        // This is used to allow event ID collisions between readonly and active executions
        to_hash.push(!execution_context.read_only as u8);
        // Hash the seed to generate the ID
        let id = SCOutputEventId(Hash::compute_from(&to_hash));

        // Gather contextual information from the execution context
        let context = EventExecutionContext {
            slot: execution_context.slot,
            block: execution_context.opt_block_id,
            call_stack: execution_context.stack.iter().map(|e| e.address).collect(),
            read_only: execution_context.read_only,
            index_in_slot: execution_context.created_event_index,
            origin_operation_id: execution_context.origin_operation_id,
        };

        // Generate the event
        let event = SCOutputEvent { id, context, data };

        // Increment the event counter fot this slot
        execution_context.created_event_index += 1;

        // Add the event to the context store
        execution_context.events.insert(id, event);

        Ok(())
    }

    /// Returns the current time (millisecond unix timestamp)
    /// Note that in order to ensure determinism, this is actually the time of the context slot.
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

    /// Returns a pseudo-random deterministic i64 number
    ///
    /// # Warning
    /// This random number generator is unsafe:
    /// it can be both predicted and manipulated before the execution
    fn unsafe_random(&self) -> Result<i64> {
        let distr = rand::distributions::Uniform::new_inclusive(i64::MIN, i64::MAX);
        Ok(context_guard!(self).unsafe_rng.sample(distr))
    }
}
