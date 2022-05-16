// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Implementation of the interface between massa-execution-worker and massa-sc-runtime.
//! This allows the VM runtime to access the Massa execution context,
//! for example to interact with the ledger.
//! See the definition of Interface in the massa-sc-runtime crate for functional details.

use crate::context::ExecutionContext;
use anyhow::{bail, Result};
use massa_async_pool::AsyncMessage;
use massa_execution_exports::ExecutionConfig;
use massa_execution_exports::ExecutionStackElement;
use massa_models::{timeslots::get_block_slot_timestamp, Address, Amount, Slot};
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
    /// execution configuration
    config: ExecutionConfig,
    /// thread-safe shared access to the execution context (see context.rs)
    context: Arc<Mutex<ExecutionContext>>,
}

impl InterfaceImpl {
    /// creates a new `InterfaceImpl`
    ///
    /// # Arguments
    /// * `config`: execution configuration
    /// * `context`: thread-safe shared access to the current execution context (see context.rs)
    pub fn new(config: ExecutionConfig, context: Arc<Mutex<ExecutionContext>>) -> InterfaceImpl {
        InterfaceImpl { config, context }
    }
}

impl InterfaceClone for InterfaceImpl {
    /// allows cloning a boxed `InterfaceImpl`
    fn clone_box(&self) -> Box<dyn Interface> {
        Box::new(self.clone())
    }
}

/// Implementation of the Interface trait providing functions for massa-sc-runtime to call
/// in order to interact with the execution context during bytecode execution.
/// See the massa-sc-runtime crate for a functional description of the trait and its methods.
/// Note that massa-sc-runtime uses basic types (`str` for addresses, `u64` for amounts...) for genericity.
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
    /// * `address`: string representation of the target address on which the bytecode will be called
    /// * `raw_coins`: raw representation (without decimal factor) of the amount of parallel coins to transfer from the caller address to the target address at the beginning of the call
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
        let key = massa_hash::Hash::compute_from(key.as_bytes());
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
        let key = massa_hash::Hash::compute_from(key.as_bytes());
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
        let key = massa_hash::Hash::compute_from(key.as_bytes());
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
        let key = massa_hash::Hash::compute_from(key.as_bytes());
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
        let key = massa_hash::Hash::compute_from(key.as_bytes());
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
        let key = massa_hash::Hash::compute_from(key.as_bytes());
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        Ok(context.has_data_entry(&addr, &key))
    }

    /// Hashes arbitrary data
    ///
    /// # Arguments
    /// * data: data bytes to hash
    ///
    /// # Returns
    /// The string representation of the resulting hash
    fn hash(&self, data: &[u8]) -> Result<String> {
        Ok(massa_hash::Hash::compute_from(data).to_bs58_check())
    }

    /// Converts a public key to an address
    ///
    /// # Arguments
    /// * `public_key`: string representation of the public key
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
        let h = massa_hash::Hash::compute_from(data);
        Ok(massa_signature::verify_signature(&h, &signature, &public_key).is_ok())
    }

    /// Transfer parallel coins from the current address (top of the call stack) towards a target address.
    ///
    /// # Arguments
    /// * `to_address`: string representation of the address to which the coins are sent
    /// * `raw_amount`: raw representation (no decimal factor) of the amount of coins to transfer
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
    /// * `from_address`: string representation of the address that is sending the coins
    /// * `to_address`: string representation of the address to which the coins are sent
    /// * `raw_amount`: raw representation (no decimal factor) of the amount of coins to transfer
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

    /// Returns the list of owned addresses (top of the call stack).
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

    /// Gets the amount of coins that have been transferred at the beginning of the call.
    /// See the `init_call` method.
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
        context_guard!(self).generate_event(data)?;
        Ok(())
    }

    /// Returns the current time (millisecond UNIX timestamp)
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

    /// Returns a pseudo-random deterministic `i64` number
    ///
    /// # Warning
    /// This random number generator is unsafe:
    /// it can be both predicted and manipulated before the execution
    fn unsafe_random(&self) -> Result<i64> {
        let distr = rand::distributions::Uniform::new_inclusive(i64::MIN, i64::MAX);
        Ok(context_guard!(self).unsafe_rng.sample(distr))
    }

    /// Adds an asynchronous message to the context speculative asynchronous pool
    ///
    /// # Arguments
    /// * `target_address`: Destination address hash in format string
    /// * `target_handler`: Name of the message handling function
    /// * `validity_start`: Tuple containing the period and thread of the validity start slot
    /// * `validity_end`: Tuple containing the period and thread of the validity end slot
    /// * `max_gas`: Maximum gas for the message execution
    /// * `gas_price`: Price of one gas unit
    /// * `raw_coins`: Coins given by the sender
    /// * `data`: Message data
    fn send_message(
        &self,
        target_address: &str,
        target_handler: &str,
        validity_start: (u64, u8),
        validity_end: (u64, u8),
        max_gas: u64,
        gas_price: u64,
        raw_coins: u64,
        data: &[u8],
    ) -> Result<()> {
        if validity_start.1 >= self.config.thread_count {
            bail!("validity start thread exceeds the configuration thread count")
        }
        if validity_end.1 >= self.config.thread_count {
            bail!("validity end thread exceeds the configuration thread count")
        }
        let mut execution_context = context_guard!(self);
        let emission_slot = execution_context.slot;
        let emission_index = execution_context.created_message_index;
        let sender = execution_context.get_current_address()?;
        execution_context.push_new_message(AsyncMessage {
            emission_slot,
            emission_index,
            sender,
            destination: Address::from_str(target_address)?,
            handler: target_handler.to_string(),
            validity_start: Slot::new(validity_start.0, validity_start.1),
            validity_end: Slot::new(validity_end.0, validity_end.1),
            max_gas,
            gas_price: Amount::from_raw(gas_price),
            coins: Amount::from_raw(raw_coins),
            data: data.to_vec(),
        });
        execution_context.created_message_index += 1;
        Ok(())
    }

    /// Returns the period of the current execution slot
    fn get_current_period(&self) -> Result<u64> {
        let slot = context_guard!(self).slot;
        Ok(slot.period)
    }

    /// Returns the thread of the current execution slot
    fn get_current_thread(&self) -> Result<u8> {
        let slot = context_guard!(self).slot;
        Ok(slot.thread)
    }

    /// Sets the bytecode of the current address
    fn raw_set_bytecode(&self, bytecode: &[u8]) -> Result<()> {
        let mut execution_context = context_guard!(self);
        let address = execution_context.get_current_address()?;
        match execution_context.set_bytecode(&address, bytecode.to_vec()) {
            Ok(()) => Ok(()),
            Err(err) => bail!("couldn't set address {} bytecode: {}", address, err),
        }
    }

    /// Sets the bytecode of an arbitrary address.
    /// Fails if the address does not exist of if the context doesn't have write access rights on it.
    fn raw_set_bytecode_for(&self, address: &str, bytecode: &[u8]) -> Result<()> {
        let address = massa_models::Address::from_str(address)?;
        let mut execution_context = context_guard!(self);
        match execution_context.set_bytecode(&address, bytecode.to_vec()) {
            Ok(()) => Ok(()),
            Err(err) => bail!("couldn't set address {} bytecode: {}", address, err),
        }
    }
}
