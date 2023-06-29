// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Implementation of the interface between massa-execution-worker and massa-sc-runtime.
//! This allows the VM runtime to access the Massa execution context,
//! for example to interact with the ledger.
//! See the definition of Interface in the massa-sc-runtime crate for functional details.

use crate::context::ExecutionContext;
use anyhow::{anyhow, bail, Result};
use massa_async_pool::{AsyncMessage, AsyncMessageTrigger};
use massa_execution_exports::ExecutionConfig;
use massa_execution_exports::ExecutionStackElement;
use massa_models::bytecode::Bytecode;
use massa_models::config::MAX_DATASTORE_KEY_LENGTH;
use massa_models::{
    address::Address, amount::Amount, slot::Slot, timeslots::get_block_slot_timestamp,
};
use massa_sc_runtime::RuntimeModule;
use massa_sc_runtime::{Interface, InterfaceClone};
use parking_lot::Mutex;
use rand::Rng;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;

#[cfg(any(
    feature = "gas_calibration",
    feature = "benchmarking",
    feature = "testing"
))]
use massa_models::datastore::Datastore;

/// helper for locking the context mutex
macro_rules! context_guard {
    ($self:ident) => {
        $self.context.lock()
    };
}

/// an implementation of the Interface trait (see massa-sc-runtime crate)
#[derive(Clone)]
pub struct InterfaceImpl {
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

    #[cfg(any(
        feature = "gas_calibration",
        feature = "benchmarking",
        feature = "testing"
    ))]
    /// Used to create an default interface to run SC in a test environment
    pub fn new_default(
        sender_addr: Address,
        operation_datastore: Option<Datastore>,
    ) -> InterfaceImpl {
        use massa_ledger_exports::{LedgerEntry, SetUpdateOrDelete};
        use massa_models::config::{
            MIP_STORE_STATS_BLOCK_CONSIDERED, MIP_STORE_STATS_COUNTERS_MAX,
        };
        use massa_module_cache::{config::ModuleCacheConfig, controller::ModuleCache};
        use massa_versioning::versioning::{MipStatsConfig, MipStore};
        use parking_lot::RwLock;

        let vesting_file = super::tests::get_initials_vesting(false);
        let config = ExecutionConfig::default();
        let (final_state, _tempfile, _tempdir) = super::tests::get_sample_state(0).unwrap();
        let module_cache = Arc::new(RwLock::new(ModuleCache::new(ModuleCacheConfig {
            hd_cache_path: config.hd_cache_path.clone(),
            gas_costs: config.gas_costs.clone(),
            compilation_gas: config.max_gas_per_block,
            lru_cache_size: config.lru_cache_size,
            hd_cache_size: config.hd_cache_size,
            snip_amount: config.snip_amount,
        })));
        let vesting_manager = Arc::new(
            crate::vesting_manager::VestingManager::new(
                config.thread_count,
                config.t0,
                config.genesis_timestamp,
                config.periods_per_cycle,
                config.roll_price,
                vesting_file.path().to_path_buf(),
            )
            .unwrap(),
        );

        // create an empty default store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            counters_max: MIP_STORE_STATS_COUNTERS_MAX,
        };
        let mip_store =
            MipStore::try_from(([], mip_stats_config)).expect("Cannot create an empty MIP store");

        let mut execution_context = ExecutionContext::new(
            config.clone(),
            final_state,
            Default::default(),
            module_cache,
            vesting_manager,
            mip_store,
        );
        execution_context.stack = vec![ExecutionStackElement {
            address: sender_addr,
            coins: Amount::zero(),
            owned_addresses: vec![sender_addr],
            operation_datastore,
        }];
        execution_context.speculative_ledger.added_changes.0.insert(
            sender_addr,
            SetUpdateOrDelete::Set(LedgerEntry {
                balance: Amount::const_init(1_000_000_000, 0),
                ..Default::default()
            }),
        );
        let context = Arc::new(Mutex::new(execution_context));
        InterfaceImpl::new(config, context)
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
    /// Initialize the call when bytecode calls a function from another bytecode
    /// This function transfers the coins passed as parameter,
    /// prepares the current execution context by pushing a new element on the top of the call stack,
    /// and returns the target bytecode from the ledger.
    ///
    /// # Arguments
    /// * `address`: string representation of the target address on which the bytecode will be called
    /// * `raw_coins`: raw representation (without decimal factor) of the amount of coins to transfer from the caller address to the target address at the beginning of the call
    ///
    /// # Returns
    /// The target bytecode or an error
    fn init_call(&self, address: &str, raw_coins: u64) -> Result<Vec<u8>> {
        // get target address
        let to_address = Address::from_str(address)?;

        // check that the target address is an SC address
        if !matches!(to_address, Address::SC(..)) {
            bail!("called address {} is not an SC address", to_address);
        }

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
        let coins = Amount::from_raw(raw_coins);
        // note: rights are not checked here we checked that to_address is an SC address above
        // and we know that the sender is at the top of the call stack
        if let Err(err) = context.transfer_coins(Some(from_address), Some(to_address), coins, false)
        {
            bail!(
                "error transferring {} coins from {} to {}: {}",
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
            operation_datastore: None,
        });

        // return the target bytecode
        Ok(bytecode.0)
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

    /// Get the module from cache if possible, compile it if not
    ///
    /// # Returns
    /// A `massa-sc-runtime` compiled module
    fn get_module(&self, bytecode: &[u8], limit: u64) -> Result<RuntimeModule> {
        let context = context_guard!(self);
        let module = context.module_cache.write().load_module(bytecode, limit)?;
        Ok(module)
    }

    /// Gets the balance of the current address address (top of the stack).
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the balance of the address,
    /// or zero if the address is not found in the ledger.
    fn get_balance(&self) -> Result<u64> {
        let context = context_guard!(self);
        let address = context.get_current_address()?;
        Ok(context.get_balance(&address).unwrap_or_default().to_raw())
    }

    /// Gets the balance of arbitrary address passed as argument.
    ///
    /// # Arguments
    /// * address: string representation of the address for which to get the balance
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the balance of the address,
    /// or zero if the address is not found in the ledger.
    fn get_balance_for(&self, address: &str) -> Result<u64> {
        let address = massa_models::address::Address::from_str(address)?;
        Ok(context_guard!(self)
            .get_balance(&address)
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
        match context_guard!(self).create_new_sc_address(Bytecode(bytecode.to_vec())) {
            Ok(addr) => Ok(addr.to_string()),
            Err(err) => bail!("couldn't create new SC address: {}", err),
        }
    }

    /// Get the datastore keys (aka entries) for a given address
    ///
    /// # Returns
    /// A list of keys (keys are byte arrays)
    fn get_keys(&self, prefix_opt: Option<&[u8]>) -> Result<BTreeSet<Vec<u8>>> {
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        match context.get_keys(&addr, prefix_opt.unwrap_or_default()) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Get the datastore keys (aka entries) for a given address
    ///
    /// # Returns
    /// A list of keys (keys are byte arrays)
    fn get_keys_for(&self, address: &str, prefix_opt: Option<&[u8]>) -> Result<BTreeSet<Vec<u8>>> {
        let addr = &Address::from_str(address)?;
        let context = context_guard!(self);
        match context.get_keys(addr, prefix_opt.unwrap_or_default()) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Checks if a datastore entry exists for a given address.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    fn has_data_for(&self, address: &str, key: &[u8]) -> Result<bool> {
        let addr = massa_models::address::Address::from_str(address)?;
        let context = context_guard!(self);
        Ok(context.has_data_entry(&addr, key))
    }

    /// Gets a datastore value by key for the current address (top of the call stack).
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The datastore value matching the provided key, if found, otherwise an error.
    fn raw_get_data(&self, key: &[u8]) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        match context.get_data_entry(&addr, key) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Sets a datastore entry for the current address (top of the call stack).
    /// Fails if the address does not exist.
    /// Creates the entry if does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to set
    /// * value: new value to set
    fn raw_set_data(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut context = context_guard!(self);
        let addr = context.get_current_address()?;
        context.set_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Appends data to a datastore entry for the current address (top of the call stack).
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * key: string key of the datastore entry
    /// * value: value to append
    fn raw_append_data(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut ctx = context_guard!(self);
        let addr = ctx.get_current_address()?;
        ctx.append_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Deletes a datastore entry by key for the current address (top of the call stack).
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to delete
    fn raw_delete_data(&self, key: &[u8]) -> Result<()> {
        let mut ctx = context_guard!(self);
        let addr = ctx.get_current_address()?;
        ctx.delete_data_entry(&addr, key)?;
        Ok(())
    }

    /// Checks if a datastore entry exists for the current address (top of the call stack).
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    fn has_data(&self, key: &[u8]) -> Result<bool> {
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        Ok(context.has_data_entry(&addr, key))
    }

    /// Gets a datastore value by key for a given address.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The datastore value matching the provided key, if found, otherwise an error.
    fn raw_get_data_for(&self, address: &str, key: &[u8]) -> Result<Vec<u8>> {
        let addr = &massa_models::address::Address::from_str(address)?;
        let context = context_guard!(self);
        match context.get_data_entry(addr, key) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Sets a datastore entry for a given address.
    /// Fails if the address does not exist.
    /// Creates the entry if it does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to set
    /// * value: new value to set
    fn raw_set_data_for(&self, address: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let addr = massa_models::address::Address::from_str(address)?;
        let mut context = context_guard!(self);
        context.set_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Appends a value to a datastore entry for a given address.
    /// Fails if the entry or address does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry
    /// * value: value to append
    fn raw_append_data_for(&self, address: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let addr = massa_models::address::Address::from_str(address)?;
        context_guard!(self).append_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Deletes a datastore entry by key for a given address.
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to delete
    fn raw_delete_data_for(&self, address: &str, key: &[u8]) -> Result<()> {
        let addr = &massa_models::address::Address::from_str(address)?;
        context_guard!(self).delete_data_entry(addr, key)?;
        Ok(())
    }

    /// Check whether or not the caller has write access in the current context
    ///
    /// # Returns
    /// true if the caller has write access
    fn caller_has_write_access(&self) -> Result<bool> {
        let context = context_guard!(self);
        let mut call_stack_iter = context.stack.iter().rev();
        let caller_owned_addresses = if let Some(last) = call_stack_iter.next() {
            if let Some(prev_to_last) = call_stack_iter.next() {
                prev_to_last.owned_addresses.clone()
            } else {
                last.owned_addresses.clone()
            }
        } else {
            return Err(anyhow!("empty stack"));
        };
        let current_address = context.get_current_address()?;
        Ok(caller_owned_addresses.contains(&current_address))
    }

    fn raw_get_bytecode(&self) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let address = context.get_current_address()?;
        match context.get_bytecode(&address) {
            Some(bytecode) => Ok(bytecode.0),
            _ => bail!("bytecode not found"),
        }
    }

    /// Returns bytecode of the target address
    fn raw_get_bytecode_for(&self, address: &str) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let address = Address::from_str(address)?;
        match context.get_bytecode(&address) {
            Some(bytecode) => Ok(bytecode.0),
            _ => bail!("bytecode not found"),
        }
    }

    /// Get the operation datastore keys (aka entries).
    /// Note that the datastore is only accessible to the initial caller level.
    ///
    /// # Returns
    /// A list of keys (keys are byte arrays)
    fn get_op_keys(&self) -> Result<Vec<Vec<u8>>> {
        // TODO return BTreeSet<Vec<u8>>

        // TODO add prefix
        let prefix: &[u8] = &[];

        // compute prefix range
        let prefix_range = if !prefix.is_empty() {
            // compute end of prefix range
            let n_keep = prefix
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, v)| if v < &255 { Some(i + 1) } else { None })
                .unwrap_or(0);
            let mut prefix_end = prefix[..n_keep].to_vec();
            if let Some(v) = prefix_end.last_mut() {
                *v += 1;
            }
            (
                std::ops::Bound::Included(prefix.to_vec()),
                if !prefix_end.is_empty() {
                    std::ops::Bound::Excluded(prefix_end)
                } else {
                    std::ops::Bound::Unbounded
                },
            )
        } else {
            (std::ops::Bound::Unbounded, std::ops::Bound::Unbounded)
        };
        let range_ref = (prefix_range.0.as_ref(), prefix_range.1.as_ref());

        let context = context_guard!(self);
        let stack = context.stack.last().ok_or_else(|| anyhow!("No stack"))?;
        let datastore = stack
            .operation_datastore
            .as_ref()
            .ok_or_else(|| anyhow!("No datastore in stack"))?;
        let keys = datastore
            .range::<Vec<u8>, _>(range_ref)
            .map(|(k, _v)| k.clone())
            .collect();
        Ok(keys)
    }

    /// Checks if an operation datastore entry exists in the operation datastore.
    /// Note that the datastore is only accessible to the initial caller level.
    ///
    /// # Arguments
    /// * key: byte array key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the entry is matching the provided key in its operation datastore, otherwise false
    fn has_op_key(&self, key: &[u8]) -> Result<bool> {
        let context = context_guard!(self);
        let stack = context.stack.last().ok_or_else(|| anyhow!("No stack"))?;
        let datastore = stack
            .operation_datastore
            .as_ref()
            .ok_or_else(|| anyhow!("No datastore in stack"))?;
        let has_key = datastore.contains_key(key);
        Ok(has_key)
    }

    /// Gets an operation datastore value by key.
    /// Note that the datastore is only accessible to the initial caller level.
    ///
    /// # Arguments
    /// * key: byte array key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The operation datastore value matching the provided key, if found, otherwise an error.
    fn get_op_data(&self, key: &[u8]) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let stack = context.stack.last().ok_or_else(|| anyhow!("No stack"))?;
        let datastore = stack
            .operation_datastore
            .as_ref()
            .ok_or_else(|| anyhow!("No datastore in stack"))?;
        let data = datastore
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow!("Unknown key: {:?}", key));
        data
    }

    /// Hashes arbitrary data
    ///
    /// # Arguments
    /// * data: data bytes to hash
    ///
    /// # Returns
    /// The hash in bytes format
    fn hash(&self, data: &[u8]) -> Result<[u8; 32]> {
        Ok(massa_hash::Hash::compute_from(data).into_bytes())
    }

    /// Converts a public key to an address
    ///
    /// # Arguments
    /// * `public_key`: string representation of the public key
    ///
    /// # Returns
    /// The string representation of the resulting address
    fn address_from_public_key(&self, public_key: &str) -> Result<String> {
        let public_key = massa_signature::PublicKey::from_str(public_key)?;
        let addr = massa_models::address::Address::from_public_key(&public_key);
        Ok(addr.to_string())
    }

    fn validate_address(&self, address: &str) -> Result<bool> {
        Ok(massa_models::address::Address::from_str(address).is_ok())
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
        let public_key = match massa_signature::PublicKey::from_str(public_key) {
            Ok(pubk) => pubk,
            Err(_) => return Ok(false),
        };
        let h = massa_hash::Hash::compute_from(data);
        Ok(public_key.verify_signature(&h, &signature).is_ok())
    }

    /// TORO remove
    fn transfer_coins(&self, to_address: &str, raw_amount: u64) -> Result<()> {
        let to_address = Address::from_str(to_address)?;
        let amount = Amount::from_raw(raw_amount);
        let mut context = context_guard!(self);
        let from_address = context.get_current_address()?;
        context.transfer_coins(Some(from_address), Some(to_address), amount, true)?;
        Ok(())
    }

    /// Transfer coins from a given address towards a target address.
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
        let from_address = Address::from_str(from_address)?;
        let to_address = Address::from_str(to_address)?;
        let amount = Amount::from_raw(raw_amount);
        let mut context = context_guard!(self);
        context.transfer_coins(Some(from_address), Some(to_address), amount, true)?;
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
            .map(|addr| addr.to_string())
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
            .map(|addr| addr.to_string())
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
        let mut context = context_guard!(self);
        let event = context.event_create(data, false);
        context.event_emit(event);
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

    /// Returns a pseudo-random deterministic `f64` number
    ///
    /// # Warning
    /// This random number generator is unsafe:
    /// it can be both predicted and manipulated before the execution
    fn unsafe_random_f64(&self) -> Result<f64> {
        let distr = rand::distributions::Uniform::new(0f64, 1f64);
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
    /// * `fee`: Fee to pay
    /// * `raw_coins`: Coins given by the sender
    /// * `data`: Message data
    fn send_message(
        &self,
        target_address: &str,
        target_handler: &str,
        validity_start: (u64, u8),
        validity_end: (u64, u8),
        max_gas: u64,
        raw_fee: u64,
        raw_coins: u64,
        data: &[u8],
        filter: Option<(&str, Option<&[u8]>)>,
    ) -> Result<()> {
        if validity_start.1 >= self.config.thread_count {
            bail!("validity start thread exceeds the configuration thread count")
        }
        if validity_end.1 >= self.config.thread_count {
            bail!("validity end thread exceeds the configuration thread count")
        }
        let target_addr = Address::from_str(target_address)?;

        // check that the target address is an SC address
        if !matches!(target_addr, Address::SC(..)) {
            bail!("target address is not a smart contract address")
        }

        let mut execution_context = context_guard!(self);
        let emission_slot = execution_context.slot;
        let emission_index = execution_context.created_message_index;
        let sender = execution_context.get_current_address()?;
        let coins = Amount::from_raw(raw_coins);
        execution_context.transfer_coins(Some(sender), None, coins, true)?;
        let fee = Amount::from_raw(raw_fee);
        execution_context.transfer_coins(Some(sender), None, fee, true)?;
        execution_context.push_new_message(AsyncMessage::new_with_hash(
            emission_slot,
            emission_index,
            sender,
            target_addr,
            target_handler.to_string(),
            max_gas,
            fee,
            coins,
            Slot::new(validity_start.0, validity_start.1),
            Slot::new(validity_end.0, validity_end.1),
            data.to_vec(),
            filter
                .map(|(addr, key)| {
                    let datastore_key = key.map(|k| k.to_vec());
                    if let Some(ref k) = datastore_key {
                        if k.len() > MAX_DATASTORE_KEY_LENGTH as usize {
                            bail!("datastore key is too long")
                        }
                    }
                    Ok::<AsyncMessageTrigger, _>(AsyncMessageTrigger {
                        address: Address::from_str(addr)?,
                        datastore_key,
                    })
                })
                .transpose()?,
            None,
        ));
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

    /// TODO remove
    fn raw_set_bytecode(&self, bytecode: &[u8]) -> Result<()> {
        let mut execution_context = context_guard!(self);
        let address = execution_context.get_current_address()?;
        match execution_context.set_bytecode(&address, Bytecode(bytecode.to_vec())) {
            Ok(()) => Ok(()),
            Err(err) => bail!("couldn't set address {} bytecode: {}", address, err),
        }
    }

    /// Sets the bytecode of an arbitrary address.
    /// Fails if the address does not exist, is an user address, or if the context doesn't have write access rights on it.
    fn raw_set_bytecode_for(&self, address: &str, bytecode: &[u8]) -> Result<()> {
        let address = massa_models::address::Address::from_str(address)?;
        let mut execution_context = context_guard!(self);
        match execution_context.set_bytecode(&address, Bytecode(bytecode.to_vec())) {
            Ok(()) => Ok(()),
            Err(err) => bail!("couldn't set address {} bytecode: {}", address, err),
        }
    }

    /// Hashes givens byte array with sha256
    ///
    /// # Arguments
    /// * bytes: byte array to hash
    ///
    /// # Returns
    /// The vector of bytes representation of the resulting hash
    fn hash_sha256(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hasher.finalize().into();
        Ok(hash)
    }
}
