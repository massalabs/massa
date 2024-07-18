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
use massa_models::datastore::get_prefix_bounds;
use massa_models::{
    address::{Address, SCAddress, UserAddress},
    amount::Amount,
    slot::Slot,
    timeslots::get_block_slot_timestamp,
};
use massa_proto_rs::massa::model::v1::{
    AddressCategory, ComparisonResult, NativeAmount, NativeTime,
};
use massa_sc_runtime::RuntimeModule;
use massa_sc_runtime::{Interface, InterfaceClone};
use massa_signature::PublicKey;
use massa_signature::Signature;
use massa_time::MassaTime;
#[cfg(any(
    feature = "gas_calibration",
    feature = "benchmarking",
    feature = "test-exports",
    test
))]
use num::rational::Ratio;
use parking_lot::Mutex;
use rand::Rng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::str::FromStr;
use std::sync::Arc;
use tracing::debug;
use tracing::warn;

#[cfg(any(
    feature = "gas_calibration",
    feature = "benchmarking",
    feature = "test-exports",
    test
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
        feature = "test-exports",
        test
    ))]
    /// Used to create an default interface to run SC in a test environment
    pub fn new_default(
        sender_addr: Address,
        operation_datastore: Option<Datastore>,
    ) -> InterfaceImpl {
        use massa_db_exports::{MassaDBConfig, MassaDBController};
        use massa_db_worker::MassaDB;
        use massa_final_state::test_exports::get_sample_state;
        use massa_ledger_exports::{LedgerEntry, SetUpdateOrDelete};
        use massa_models::config::{MIP_STORE_STATS_BLOCK_CONSIDERED, THREAD_COUNT};
        use massa_module_cache::{config::ModuleCacheConfig, controller::ModuleCache};
        use massa_pos_exports::SelectorConfig;
        use massa_pos_worker::start_selector_worker;
        use massa_versioning::versioning::{MipStatsConfig, MipStore};
        use parking_lot::RwLock;
        use tempfile::TempDir;

        let config = ExecutionConfig::default();
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mip_store = MipStore::try_from(([], mip_stats_config)).unwrap();
        let (_, selector_controller) = start_selector_worker(SelectorConfig::default())
            .expect("could not start selector controller");
        let disk_ledger = TempDir::new().expect("cannot create temp directory");
        let db_config = MassaDBConfig {
            path: disk_ledger.path().to_path_buf(),
            max_history_length: 10,
            max_final_state_elements_size: 100_000,
            max_versioning_elements_size: 100_000,
            thread_count: THREAD_COUNT,
            max_ledger_backups: 10,
        };

        let db = Arc::new(RwLock::new(
            Box::new(MassaDB::new(db_config)) as Box<(dyn MassaDBController + 'static)>
        ));
        let (final_state, _tempfile) =
            get_sample_state(config.last_start_period, selector_controller, mip_store, db).unwrap();
        let module_cache = Arc::new(RwLock::new(ModuleCache::new(ModuleCacheConfig {
            hd_cache_path: config.hd_cache_path.clone(),
            gas_costs: config.gas_costs.clone(),
            lru_cache_size: config.lru_cache_size,
            hd_cache_size: config.hd_cache_size,
            snip_amount: config.snip_amount,
            max_module_length: config.max_bytecode_size,
        })));

        // create an empty default store
        let mip_stats_config = MipStatsConfig {
            block_count_considered: MIP_STORE_STATS_BLOCK_CONSIDERED,
            warn_announced_version_ratio: Ratio::new_raw(30, 100),
        };
        let mip_store =
            MipStore::try_from(([], mip_stats_config)).expect("Cannot create an empty MIP store");

        let mut execution_context = ExecutionContext::new(
            config.clone(),
            final_state,
            Default::default(),
            module_cache,
            mip_store,
            massa_hash::Hash::zero(),
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

/// Helper function that creates an amount from a NativeAmount
fn amount_from_native_amount(amount: &NativeAmount) -> Result<Amount> {
    let amount = Amount::from_mantissa_scale(amount.mantissa, amount.scale)
        .map_err(|err| anyhow!(format!("{}", err)))?;

    Ok(amount)
}

/// Helper function that creates a NativeAmount from the amount internal representation
fn amount_to_native_amount(amount: &Amount) -> NativeAmount {
    let (mantissa, scale) = amount.to_mantissa_scale();
    NativeAmount { mantissa, scale }
}

/// Helper function that creates an MassaTime from a NativeTime
fn massa_time_from_native_time(time: &NativeTime) -> Result<MassaTime> {
    let time = MassaTime::from_millis(time.milliseconds);
    Ok(time)
}

/// Helper function that creates a NativeTime from the MassaTime internal representation
fn massa_time_to_native_time(time: &MassaTime) -> NativeTime {
    let milliseconds = time.as_millis();
    NativeTime { milliseconds }
}

/// Helper function to get the address from the option given as argument to some ABIs
/// Fallback to the current context address if not provided.
fn get_address_from_opt_or_context(
    context: &ExecutionContext,
    option_address_string: Option<String>,
) -> Result<Address> {
    match option_address_string {
        Some(address_string) => Address::from_str(&address_string).map_err(|e| e.into()),
        None => context.get_current_address().map_err(|e| e.into()),
    }
}

/// Implementation of the Interface trait providing functions for massa-sc-runtime to call
/// in order to interact with the execution context during bytecode execution.
/// See the massa-sc-runtime crate for a functional description of the trait and its methods.
/// Note that massa-sc-runtime uses basic types (`str` for addresses, `u64` for amounts...) for genericity.
impl Interface for InterfaceImpl {
    /// prints a message in the node logs at log level 3 (debug)
    fn print(&self, message: &str) -> Result<()> {
        if cfg!(test) {
            println!("SC print: {}", message);
        } else {
            debug!("SC print: {}", message);
        }
        Ok(())
    }

    fn get_interface_version(&self) -> Result<u32> {
        let context = context_guard!(self);
        Ok(context.execution_component_version)
    }

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

        // write-lock context
        let mut context = context_guard!(self);

        // check that the target address is a SC address and if it exists
        context.check_target_sc_address(to_address)?;

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
    /// A `massa-sc-runtime` CL compiled module & the remaining gas after loading the module
    fn get_module(&self, bytecode: &[u8], gas_limit: u64) -> Result<RuntimeModule> {
        Ok((context_guard!(self))
            .module_cache
            .write()
            .load_module(bytecode, gas_limit)?)
    }

    /// Compile and return a temporary module
    ///
    /// # Returns
    /// A `massa-sc-runtime` SP compiled module & the remaining gas after loading the module
    fn get_tmp_module(&self, bytecode: &[u8], gas_limit: u64) -> Result<RuntimeModule> {
        Ok((context_guard!(self))
            .module_cache
            .write()
            .load_tmp_module(bytecode, gas_limit)?)
    }

    /// Gets the balance of the current address address (top of the stack).
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the balance of the address,
    /// or zero if the address is not found in the ledger.
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_balance_wasmv1`
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_balance_wasmv1`
    fn get_balance_for(&self, address: &str) -> Result<u64> {
        let address = massa_models::address::Address::from_str(address)?;
        Ok(context_guard!(self)
            .get_balance(&address)
            .unwrap_or_default()
            .to_raw())
    }

    /// Gets the balance of arbitrary address passed as argument, or the balance of the current address if no argument is passed.
    ///
    /// # Arguments
    /// * address: string representation of the address for which to get the balance
    ///
    /// # Returns
    /// The raw representation (no decimal factor) of the balance of the address,
    /// or zero if the address is not found in the ledger.
    fn get_balance_wasmv1(&self, address: Option<String>) -> Result<NativeAmount> {
        let context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        let amount = context.get_balance(&address).unwrap_or_default();
        let native_amount = amount_to_native_amount(&amount);

        Ok(native_amount)
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_keys_wasmv1`
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_keys_wasmv1`
    fn get_keys_for(&self, address: &str, prefix_opt: Option<&[u8]>) -> Result<BTreeSet<Vec<u8>>> {
        let addr = &Address::from_str(address)?;
        let context = context_guard!(self);
        match context.get_keys(addr, prefix_opt.unwrap_or_default()) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Get the datastore keys (aka entries) for a given address, or the current address if none is provided
    ///
    /// # Returns
    /// A list of keys (keys are byte arrays)
    fn get_ds_keys_wasmv1(
        &self,
        prefix: &[u8],
        address: Option<String>,
    ) -> Result<BTreeSet<Vec<u8>>> {
        let context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        match context.get_keys(&address, prefix) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Gets a datastore value by key for the current address (top of the call stack).
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The datastore value matching the provided key, if found, otherwise an error.
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_get_data_wasmv1`
    fn raw_get_data(&self, key: &[u8]) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        match context.get_data_entry(&addr, key) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_get_data_wasmv1`
    fn raw_get_data_for(&self, address: &str, key: &[u8]) -> Result<Vec<u8>> {
        let addr = &massa_models::address::Address::from_str(address)?;
        let context = context_guard!(self);
        match context.get_data_entry(addr, key) {
            Some(value) => Ok(value),
            _ => bail!("data entry not found"),
        }
    }

    /// Gets a datastore value by key for a given address, or the current address if none is provided.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// The datastore value matching the provided key, if found, otherwise an error.
    fn get_ds_value_wasmv1(&self, key: &[u8], address: Option<String>) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        match context.get_data_entry(&address, key) {
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_set_data_wasmv1`
    fn raw_set_data(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut context = context_guard!(self);
        let addr = context.get_current_address()?;
        context.set_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Sets a datastore entry for a given address.
    /// Fails if the address does not exist.
    /// Creates the entry if it does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to set
    /// * value: new value to set
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_set_data_wasmv1`
    fn raw_set_data_for(&self, address: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let addr = massa_models::address::Address::from_str(address)?;
        let mut context = context_guard!(self);
        context.set_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    fn set_ds_value_wasmv1(&self, key: &[u8], value: &[u8], address: Option<String>) -> Result<()> {
        let mut context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        context.set_data_entry(&address, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Appends data to a datastore entry for the current address (top of the call stack).
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * key: string key of the datastore entry
    /// * value: value to append
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_append_data_wasmv1`
    fn raw_append_data(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let mut context = context_guard!(self);
        let addr = context.get_current_address()?;
        context.append_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Appends a value to a datastore entry for a given address.
    /// Fails if the entry or address does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry
    /// * value: value to append
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_append_data_wasmv1`
    fn raw_append_data_for(&self, address: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let addr = massa_models::address::Address::from_str(address)?;
        context_guard!(self).append_data_entry(&addr, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Appends a value to a datastore entry for a given address, or the current address if none is provided
    /// Fails if the entry or address does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry
    /// * value: value to append
    fn append_ds_value_wasmv1(
        &self,
        key: &[u8],
        value: &[u8],
        address: Option<String>,
    ) -> Result<()> {
        let mut context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        context.append_data_entry(&address, key.to_vec(), value.to_vec())?;
        Ok(())
    }

    /// Deletes a datastore entry by key for the current address (top of the call stack).
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to delete
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_delete_data_wasmv1`
    fn raw_delete_data(&self, key: &[u8]) -> Result<()> {
        let mut context = context_guard!(self);
        let addr = context.get_current_address()?;
        context.delete_data_entry(&addr, key)?;
        Ok(())
    }

    /// Deletes a datastore entry by key for a given address.
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to delete
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_delete_data_wasmv1`
    fn raw_delete_data_for(&self, address: &str, key: &[u8]) -> Result<()> {
        let addr = &massa_models::address::Address::from_str(address)?;
        context_guard!(self).delete_data_entry(addr, key)?;
        Ok(())
    }

    /// Deletes a datastore entry by key for a given address, or the current address if none is provided.
    /// Fails if the address or entry does not exist.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to delete
    fn delete_ds_entry_wasmv1(&self, key: &[u8], address: Option<String>) -> Result<()> {
        let mut context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        context.delete_data_entry(&address, key)?;
        Ok(())
    }

    /// Checks if a datastore entry exists for the current address (top of the call stack).
    ///
    /// # Arguments
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    ///
    /// [DeprecatedByNewRuntime] Replaced by `has_data_wasmv1`
    fn has_data(&self, key: &[u8]) -> Result<bool> {
        let context = context_guard!(self);
        let addr = context.get_current_address()?;
        Ok(context.has_data_entry(&addr, key))
    }

    /// Checks if a datastore entry exists for a given address.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    ///
    /// [DeprecatedByNewRuntime] Replaced by `has_data_wasmv1`
    fn has_data_for(&self, address: &str, key: &[u8]) -> Result<bool> {
        let addr = massa_models::address::Address::from_str(address)?;
        let context = context_guard!(self);
        Ok(context.has_data_entry(&addr, key))
    }

    /// Checks if a datastore entry exists for a given address, or the current address if none is provided.
    ///
    /// # Arguments
    /// * address: string representation of the address
    /// * key: string key of the datastore entry to retrieve
    ///
    /// # Returns
    /// true if the address exists and has the entry matching the provided key in its datastore, otherwise false
    fn ds_entry_exists_wasmv1(&self, key: &[u8], address: Option<String>) -> Result<bool> {
        let context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        Ok(context.has_data_entry(&address, key))
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

    /// Returns bytecode of the current address
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_get_bytecode_wasmv1`
    fn raw_get_bytecode(&self) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let address = context.get_current_address()?;
        match context.get_bytecode(&address) {
            Some(bytecode) => Ok(bytecode.0),
            _ => bail!("bytecode not found"),
        }
    }

    /// Returns bytecode of the target address
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_get_bytecode_wasmv1`
    fn raw_get_bytecode_for(&self, address: &str) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let address = Address::from_str(address)?;
        match context.get_bytecode(&address) {
            Some(bytecode) => Ok(bytecode.0),
            _ => bail!("bytecode not found"),
        }
    }

    /// Returns bytecode of the target address, or the current address if not provided
    fn get_bytecode_wasmv1(&self, address: Option<String>) -> Result<Vec<u8>> {
        let context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_op_keys_wasmv1`
    fn get_op_keys(&self, prefix_opt: Option<&[u8]>) -> Result<Vec<Vec<u8>>> {
        let prefix: &[u8] = prefix_opt.unwrap_or_default();

        // compute prefix range
        let prefix_range = get_prefix_bounds(prefix);
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

    /// Get the operation datastore keys (aka entries).
    /// Note that the datastore is only accessible to the initial caller level.
    ///
    /// # Returns
    /// A list of keys (keys are byte arrays) that match the given prefix
    fn get_op_keys_wasmv1(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
        let prefix_range = get_prefix_bounds(prefix);
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
    fn op_entry_exists(&self, key: &[u8]) -> Result<bool> {
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

    /// Verify an EVM signature
    ///
    /// Information:
    /// * Expects a SECP256K1 signature in full ETH format.
    ///   Format: (r, s, v) v will be ignored
    ///   Length: 65 bytes
    /// * Expects a public key in raw secp256k1 format.
    ///   Length: 64 bytes
    fn evm_signature_verify(
        &self,
        message_: &[u8],
        signature_: &[u8],
        public_key_: &[u8],
    ) -> Result<bool> {
        // check the signature length
        if signature_.len() != 65 {
            return Err(anyhow!("invalid signature length in evm_signature_verify"));
        }

        // parse the public key
        let public_key = libsecp256k1::PublicKey::parse_slice(
            public_key_,
            Some(libsecp256k1::PublicKeyFormat::Raw),
        )?;

        // build the message
        let prefix = format!("\x19Ethereum Signed Message:\n{}", message_.len());
        let to_hash = [prefix.as_bytes(), message_].concat();
        let full_hash = sha3::Keccak256::digest(to_hash);
        let message = libsecp256k1::Message::parse_slice(&full_hash)
            .expect("message could not be parsed from a hash slice");

        // parse the signature as being (r, s, v)
        // r is the R.x value of the signature's R point (32 bytes)
        // s is the signature proof for R.x (32 bytes)
        // v is a recovery parameter used to ease the signature verification (1 byte)
        // we ignore the recovery parameter here
        // see test_evm_verify for an example of its usage
        let signature = libsecp256k1::Signature::parse_standard_slice(&signature_[..64])?;

        // verify the signature
        Ok(libsecp256k1::verify(&message, &signature, &public_key))
    }

    /// Keccak256 hash function
    fn hash_keccak256(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        Ok(sha3::Keccak256::digest(bytes).into())
    }

    /// Get an EVM address from a raw secp256k1 public key (64 bytes).
    /// Address is the last 20 bytes of the hash of the public key.
    fn evm_get_address_from_pubkey(&self, public_key_: &[u8]) -> Result<Vec<u8>> {
        // parse the public key
        let public_key = libsecp256k1::PublicKey::parse_slice(
            public_key_,
            Some(libsecp256k1::PublicKeyFormat::Raw),
        )?;

        // compute the hash of the public key
        let hash = sha3::Keccak256::digest(public_key.serialize());

        // ignore the first 12 bytes of the hash
        let address = hash[12..].to_vec();

        // return the address (last 20 bytes of the hash)
        Ok(address)
    }

    /// Get a raw secp256k1 public key from an EVM signature and the signed hash.
    fn evm_get_pubkey_from_signature(&self, hash_: &[u8], signature_: &[u8]) -> Result<Vec<u8>> {
        // check the signature length
        if signature_.len() != 65 {
            return Err(anyhow!(
                "invalid signature length in evm_get_pubkey_from_signature"
            ));
        }

        // parse the message
        let message = libsecp256k1::Message::parse_slice(hash_).unwrap();

        // parse the signature as being (r, s, v) use only r and s
        let signature = libsecp256k1::Signature::parse_standard_slice(&signature_[..64]).unwrap();

        // parse v as a recovery id
        let recovery_id = libsecp256k1::RecoveryId::parse_rpc(signature_[64]).unwrap();

        // recover the public key
        let recovered = libsecp256k1::recover(&message, &signature, &recovery_id).unwrap();

        // return its serialized value
        Ok(recovered.serialize().to_vec())
    }

    // Return true if the address is a User address, false if it is an SC address.
    fn is_address_eoa(&self, address_: &str) -> Result<bool> {
        let address = Address::from_str(address_)?;
        Ok(matches!(address, Address::User(..)))
    }

    /// Transfer coins from the current address (top of the call stack) towards a target address.
    ///
    /// # Arguments
    /// * `to_address`: string representation of the address to which the coins are sent
    /// * `raw_amount`: raw representation (no decimal factor) of the amount of coins to transfer
    ///
    /// [DeprecatedByNewRuntime] Replaced by `transfer_coins_wasmv1`
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `transfer_coins_wasmv1`
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

    /// Transfer coins from a given address (or the current address if not specified) towards a target address.
    ///
    /// # Arguments
    /// * `to_address`: string representation of the address to which the coins are sent
    /// * `raw_amount`: raw representation (no decimal factor) of the amount of coins to transfer
    /// * `from_address`: string representation of the address that is sending the coins
    fn transfer_coins_wasmv1(
        &self,
        to_address: String,
        raw_amount: NativeAmount,
        from_address: Option<String>,
    ) -> Result<()> {
        let to_address = Address::from_str(&to_address)?;
        let amount = amount_from_native_amount(&raw_amount)?;

        let mut context = context_guard!(self);
        let from_address = match from_address {
            Some(from_address) => Address::from_str(&from_address)?,
            None => context.get_current_address()?,
        };
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_call_coins_wasmv1`
    fn get_call_coins(&self) -> Result<u64> {
        Ok(context_guard!(self).get_current_call_coins()?.to_raw())
    }

    /// Gets the amount of coins that have been transferred at the beginning of the call.
    /// See the `init_call` method.
    ///
    /// # Returns
    /// The amount of coins
    fn get_call_coins_wasmv1(&self) -> Result<NativeAmount> {
        let amount = context_guard!(self).get_current_call_coins()?;
        Ok(amount_to_native_amount(&amount))
    }

    /// Emits an execution event to be stored.
    ///
    /// # Arguments:
    /// data: the string data that is the payload of the event
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_current_slot`
    fn generate_event(&self, data: String) -> Result<()> {
        if data.len() > self.config.max_event_size {
            bail!("Event data size is too large");
        };

        let mut context = context_guard!(self);
        let event = context.event_create(data, false);
        context.event_emit(event);
        Ok(())
    }

    /// Emits an execution event to be stored.
    ///
    /// # Arguments:
    /// data: the bytes_array data that is the payload of the event
    fn generate_event_wasmv1(&self, data: Vec<u8>) -> Result<()> {
        if data.len() > self.config.max_event_size {
            bail!("Event data size is too large");
        };

        let data_str = String::from_utf8(data.clone()).unwrap_or(format!("{:?}", data));
        let mut context = context_guard!(self);
        let event = context.event_create(data_str, false);
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
        Ok(ts.as_millis())
    }

    /// Returns a pseudo-random deterministic `i64` number
    ///
    /// # Warning
    /// This random number generator is unsafe:
    /// it can be both predicted and manipulated before the execution
    ///
    /// [DeprecatedByNewRuntime] Replaced by `unsafe_random_wasmv1`
    fn unsafe_random(&self) -> Result<i64> {
        let distr = rand::distributions::Uniform::new_inclusive(i64::MIN, i64::MAX);
        Ok(context_guard!(self).unsafe_rng.sample(distr))
    }

    /// Returns a pseudo-random deterministic `f64` number
    ///
    /// # Warning
    /// This random number generator is unsafe:
    /// it can be both predicted and manipulated before the execution
    ///
    /// [DeprecatedByNewRuntime] Replaced by `unsafe_random_wasmv1`
    fn unsafe_random_f64(&self) -> Result<f64> {
        let distr = rand::distributions::Uniform::new(0f64, 1f64);
        Ok(context_guard!(self).unsafe_rng.sample(distr))
    }

    /// Returns a pseudo-random deterministic byte array, with the given number of bytes
    ///
    /// # Warning
    /// This random number generator is unsafe:
    /// it can be both predicted and manipulated before the execution
    fn unsafe_random_wasmv1(&self, num_bytes: u64) -> Result<Vec<u8>> {
        let mut arr = vec![0u8; num_bytes as usize];
        context_guard!(self).unsafe_rng.try_fill_bytes(&mut arr)?;
        Ok(arr)
    }

    /// Adds an asynchronous message to the context speculative asynchronous pool
    ///
    /// # Arguments
    /// * `target_address`: Destination address hash in format string
    /// * `target_function`: Name of the message handling function
    /// * `validity_start`: Tuple containing the period and thread of the validity start slot
    /// * `validity_end`: Tuple containing the period and thread of the validity end slot
    /// * `max_gas`: Maximum gas for the message execution
    /// * `fee`: Fee to pay
    /// * `raw_coins`: Coins given by the sender
    /// * `data`: Message data
    fn send_message(
        &self,
        target_address: &str,
        target_function: &str,
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

        // Length verifications
        if target_function.len() > self.config.max_function_length as usize {
            bail!("Function name is too large");
        }
        if data.len() > self.config.max_parameter_length as usize {
            bail!("Parameter size is too large");
        }

        let mut execution_context = context_guard!(self);
        let emission_slot = execution_context.slot;

        let execution_component_version = execution_context.execution_component_version;
        if execution_component_version > 0 {
            if max_gas < self.config.gas_costs.max_instance_cost {
                bail!("max gas is lower than the minimum instance cost")
            }
            if Slot::new(validity_end.0, validity_end.1)
                < Slot::new(validity_start.0, validity_start.1)
            {
                bail!("validity end is earlier than the validity start")
            }
            if Slot::new(validity_end.0, validity_end.1) < emission_slot {
                bail!("validity end is earlier than the current slot")
            }
        }

        let emission_index = execution_context.created_message_index;
        let sender = execution_context.get_current_address()?;
        let coins = Amount::from_raw(raw_coins);
        execution_context.transfer_coins(Some(sender), None, coins, true)?;
        let fee = Amount::from_raw(raw_fee);
        execution_context.transfer_coins(Some(sender), None, fee, true)?;
        execution_context.push_new_message(AsyncMessage::new(
            emission_slot,
            emission_index,
            sender,
            target_addr,
            target_function.to_string(),
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
                        if k.len() > self.config.max_datastore_key_length as usize {
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

    // Returns the operation id that originated the current execution if there is one
    fn get_origin_operation_id(&self) -> Result<Option<String>> {
        let operation_id = context_guard!(self)
            .origin_operation_id
            .map(|op_id| op_id.to_string());
        Ok(operation_id)
    }

    /// Returns the period of the current execution slot
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_current_slot`
    fn get_current_period(&self) -> Result<u64> {
        let slot = context_guard!(self).slot;
        Ok(slot.period)
    }

    /// Returns the thread of the current execution slot
    ///
    /// [DeprecatedByNewRuntime] Replaced by `get_current_slot`
    fn get_current_thread(&self) -> Result<u8> {
        let slot = context_guard!(self).slot;
        Ok(slot.thread)
    }

    /// Returns the current execution slot
    fn get_current_slot(&self) -> Result<massa_proto_rs::massa::model::v1::Slot> {
        let slot_models = context_guard!(self).slot;
        Ok(slot_models.into())
    }

    /// Sets the bytecode of the current address
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_set_bytecode_wasmv1`
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
    ///
    /// [DeprecatedByNewRuntime] Replaced by `raw_set_bytecode_wasmv1`
    fn raw_set_bytecode_for(&self, address: &str, bytecode: &[u8]) -> Result<()> {
        let address: Address = massa_models::address::Address::from_str(address)?;
        let mut execution_context = context_guard!(self);
        match execution_context.set_bytecode(&address, Bytecode(bytecode.to_vec())) {
            Ok(()) => Ok(()),
            Err(err) => bail!("couldn't set address {} bytecode: {}", address, err),
        }
    }

    /// Sets the bytecode of an arbitrary address, or the current address if not provided.
    /// Fails if the address does not exist, is an user address, or if the context doesn't have write access rights on it.
    fn set_bytecode_wasmv1(&self, bytecode: &[u8], address: Option<String>) -> Result<()> {
        let mut context = context_guard!(self);
        let address = get_address_from_opt_or_context(&context, address)?;

        match context.set_bytecode(&address, Bytecode(bytecode.to_vec())) {
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
    /// The byte array of the resulting hash
    fn hash_sha256(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let hash = hasher.finalize().into();
        Ok(hash)
    }

    /// Hashes givens byte array with blake3
    ///
    /// # Arguments
    /// * bytes: byte array to hash
    ///
    /// # Returns
    /// The byte array of the resulting hash
    fn hash_blake3(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        Ok(blake3::hash(bytes).into())
    }

    #[allow(unused_variables)]
    fn init_call_wasmv1(&self, address: &str, raw_coins: NativeAmount) -> Result<Vec<u8>> {
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
        let coins = amount_from_native_amount(&raw_coins)?;
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

    /// Returns a NativeAmount from a string
    fn native_amount_from_str_wasmv1(&self, amount: &str) -> Result<NativeAmount> {
        let amount = Amount::from_str(amount).map_err(|err| anyhow!(format!("{}", err)))?;
        Ok(amount_to_native_amount(&amount))
    }

    /// Returns a string from a NativeAmount
    fn native_amount_to_string_wasmv1(&self, amount: &NativeAmount) -> Result<String> {
        let amount = amount_from_native_amount(amount)
            .map_err(|err| anyhow!(format!("Couldn't convert native amount to Amount: {}", err)))?;
        Ok(amount.to_string())
    }

    /// Checks if the given native amount is valid
    fn check_native_amount_wasmv1(&self, amount: &NativeAmount) -> Result<bool> {
        Ok(amount_from_native_amount(amount).is_ok())
    }

    /// Adds two native amounts, saturating at the numeric bounds instead of overflowing.
    fn add_native_amount_wasmv1(
        &self,
        amount1: &NativeAmount,
        amount2: &NativeAmount,
    ) -> Result<NativeAmount> {
        let amount1 = amount_from_native_amount(amount1)?;
        let amount2 = amount_from_native_amount(amount2)?;
        let sum = amount1.saturating_add(amount2);
        Ok(amount_to_native_amount(&sum))
    }

    /// Subtracts two native amounts, saturating at the numeric bounds instead of overflowing.
    fn sub_native_amount_wasmv1(
        &self,
        amount1: &NativeAmount,
        amount2: &NativeAmount,
    ) -> Result<NativeAmount> {
        let amount1 = amount_from_native_amount(amount1)?;
        let amount2 = amount_from_native_amount(amount2)?;
        let sub = amount1.saturating_sub(amount2);
        Ok(amount_to_native_amount(&sub))
    }

    /// Multiplies a native amount by a factor, saturating at the numeric bounds instead of overflowing.
    fn scalar_mul_native_amount_wasmv1(
        &self,
        amount: &NativeAmount,
        factor: u64,
    ) -> Result<NativeAmount> {
        let amount = amount_from_native_amount(amount)?;
        let mul = amount.saturating_mul_u64(factor);
        Ok(amount_to_native_amount(&mul))
    }

    /// Divides a native amount by a divisor, return an error if the divisor is 0.
    fn scalar_div_rem_native_amount_wasmv1(
        &self,
        dividend: &NativeAmount,
        divisor: u64,
    ) -> Result<(NativeAmount, NativeAmount)> {
        let dividend = amount_from_native_amount(dividend)?;

        let quotient = dividend
            .checked_div_u64(divisor)
            .ok_or_else(|| anyhow!(format!("Couldn't div_rem native amount")))?;
        // we can unwrap, we
        let remainder = dividend
            .checked_rem_u64(divisor)
            .ok_or_else(|| anyhow!(format!("Couldn't checked_rem_u64 native amount")))?;

        Ok((
            amount_to_native_amount(&quotient),
            amount_to_native_amount(&remainder),
        ))
    }

    /// Divides a native amount by a divisor, return an error if the divisor is 0.
    fn div_rem_native_amount_wasmv1(
        &self,
        dividend: &NativeAmount,
        divisor: &NativeAmount,
    ) -> Result<(u64, NativeAmount)> {
        let dividend = amount_from_native_amount(dividend)?;
        let divisor = amount_from_native_amount(divisor)?;

        let quotient = dividend
            .checked_div(divisor)
            .ok_or_else(|| anyhow!(format!("Couldn't div_rem native amount")))?;

        let remainder = dividend
            .checked_rem(&divisor)
            .ok_or_else(|| anyhow!(format!("Couldn't checked_rem native amount")))?;
        let remainder = amount_to_native_amount(&remainder);

        Ok((quotient, remainder))
    }

    fn base58_check_to_bytes_wasmv1(&self, s: &str) -> Result<Vec<u8>> {
        bs58::decode(s)
            .with_check(None)
            .into_vec()
            .map_err(|err| anyhow!(format!("bs58 parsing error: {}", err)))
    }

    fn bytes_to_base58_check_wasmv1(&self, data: &[u8]) -> String {
        bs58::encode(data).with_check().into_string()
    }

    fn check_address_wasmv1(&self, to_check: &str) -> Result<bool> {
        Ok(Address::from_str(to_check).is_ok())
    }

    fn check_pubkey_wasmv1(&self, to_check: &str) -> Result<bool> {
        Ok(PublicKey::from_str(to_check).is_ok())
    }

    fn check_signature_wasmv1(&self, to_check: &str) -> Result<bool> {
        Ok(Signature::from_str(to_check).is_ok())
    }

    fn get_address_category_wasmv1(&self, to_check: &str) -> Result<AddressCategory> {
        let addr = Address::from_str(to_check)?;
        let execution_component_version = context_guard!(self).execution_component_version;

        match (addr, execution_component_version) {
            (Address::User(_), 0) => Ok(AddressCategory::ScAddress),
            (Address::SC(_), 0) => Ok(AddressCategory::UserAddress),
            (Address::User(_), _) => Ok(AddressCategory::UserAddress),
            (Address::SC(_), _) => Ok(AddressCategory::ScAddress),
            #[allow(unreachable_patterns)]
            (_, _) => Ok(AddressCategory::Unspecified),
        }
    }

    fn get_address_version_wasmv1(&self, address: &str) -> Result<u64> {
        let address = Address::from_str(address)?;
        match address {
            Address::User(UserAddress::UserAddressV0(_)) => Ok(0),
            // Address::User(UserAddress::UserAddressV1(_)) => Ok(1),
            Address::SC(SCAddress::SCAddressV0(_)) => Ok(0),
            // Address::SC(SCAddress::SCAddressV1(_)) => Ok(1),
            #[allow(unreachable_patterns)]
            _ => bail!("Unknown address version"),
        }
    }

    fn get_pubkey_version_wasmv1(&self, pubkey: &str) -> Result<u64> {
        let pubkey = PublicKey::from_str(pubkey)?;
        match pubkey {
            PublicKey::PublicKeyV0(_) => Ok(0),
            #[allow(unreachable_patterns)]
            _ => bail!("Unknown pubkey version"),
        }
    }

    fn get_signature_version_wasmv1(&self, signature: &str) -> Result<u64> {
        let signature = Signature::from_str(signature)?;
        match signature {
            Signature::SignatureV0(_) => Ok(0),
            #[allow(unreachable_patterns)]
            _ => bail!("Unknown signature version"),
        }
    }

    fn checked_add_native_time_wasmv1(
        &self,
        time1: &NativeTime,
        time2: &NativeTime,
    ) -> Result<NativeTime> {
        let time1 = massa_time_from_native_time(time1)?;
        let time2 = massa_time_from_native_time(time2)?;
        let sum = time1.checked_add(time2)?;
        Ok(massa_time_to_native_time(&sum))
    }

    fn checked_sub_native_time_wasmv1(
        &self,
        time1: &NativeTime,
        time2: &NativeTime,
    ) -> Result<NativeTime> {
        let time1 = massa_time_from_native_time(time1)?;
        let time2 = massa_time_from_native_time(time2)?;
        let sub = time1.checked_sub(time2)?;
        Ok(massa_time_to_native_time(&sub))
    }

    fn checked_mul_native_time_wasmv1(&self, time: &NativeTime, factor: u64) -> Result<NativeTime> {
        let time1 = massa_time_from_native_time(time)?;
        let mul = time1.checked_mul(factor)?;
        Ok(massa_time_to_native_time(&mul))
    }

    fn checked_scalar_div_native_time_wasmv1(
        &self,
        dividend: &NativeTime,
        divisor: u64,
    ) -> Result<(NativeTime, NativeTime)> {
        let dividend = massa_time_from_native_time(dividend)?;

        let quotient = dividend
            .checked_div_u64(divisor)
            .or_else(|_| bail!(format!("Couldn't div_rem native time")))?;
        let remainder = dividend
            .checked_rem_u64(divisor)
            .or_else(|_| bail!(format!("Couldn't checked_rem_u64 native time")))?;

        Ok((
            massa_time_to_native_time(&quotient),
            massa_time_to_native_time(&remainder),
        ))
    }

    fn checked_div_native_time_wasmv1(
        &self,
        dividend: &NativeTime,
        divisor: &NativeTime,
    ) -> Result<(u64, NativeTime)> {
        let dividend = massa_time_from_native_time(dividend)?;
        let divisor = massa_time_from_native_time(divisor)?;

        let quotient = dividend
            .checked_div_time(divisor)
            .or_else(|_| bail!(format!("Couldn't div_rem native time")))?;

        let remainder = dividend
            .checked_rem_time(divisor)
            .or_else(|_| bail!(format!("Couldn't checked_rem native time")))?;
        let remainder = massa_time_to_native_time(&remainder);

        Ok((quotient, remainder))
    }

    fn compare_address_wasmv1(&self, left: &str, right: &str) -> Result<ComparisonResult> {
        let left = Address::from_str(left)?;
        let right = Address::from_str(right)?;

        let res = match left.cmp(&right) {
            std::cmp::Ordering::Less => ComparisonResult::Lower,
            std::cmp::Ordering::Equal => ComparisonResult::Equal,
            std::cmp::Ordering::Greater => ComparisonResult::Greater,
        };

        Ok(res)
    }

    fn compare_native_amount_wasmv1(
        &self,
        left: &NativeAmount,
        right: &NativeAmount,
    ) -> Result<ComparisonResult> {
        let left = amount_from_native_amount(left)?;
        let right = amount_from_native_amount(right)?;

        let res = match left.cmp(&right) {
            std::cmp::Ordering::Less => ComparisonResult::Lower,
            std::cmp::Ordering::Equal => ComparisonResult::Equal,
            std::cmp::Ordering::Greater => ComparisonResult::Greater,
        };

        Ok(res)
    }

    fn compare_native_time_wasmv1(
        &self,
        left: &NativeTime,
        right: &NativeTime,
    ) -> Result<ComparisonResult> {
        let left = massa_time_from_native_time(left)?;
        let right = massa_time_from_native_time(right)?;

        let res = match left.cmp(&right) {
            std::cmp::Ordering::Less => ComparisonResult::Lower,
            std::cmp::Ordering::Equal => ComparisonResult::Equal,
            std::cmp::Ordering::Greater => ComparisonResult::Greater,
        };

        Ok(res)
    }

    fn compare_pub_key_wasmv1(&self, left: &str, right: &str) -> Result<ComparisonResult> {
        let left = PublicKey::from_str(left)?;
        let right = PublicKey::from_str(right)?;

        let res = match left.cmp(&right) {
            std::cmp::Ordering::Less => ComparisonResult::Lower,
            std::cmp::Ordering::Equal => ComparisonResult::Equal,
            std::cmp::Ordering::Greater => ComparisonResult::Greater,
        };

        Ok(res)
    }

    fn chain_id(&self) -> Result<u64> {
        Ok(self.config.chain_id)
    }

    /// Try to get a write lock on the execution context then set the
    /// gas_used_until_the_last_subexecution field to the given `gas_remaining` value.
    ///
    /// If the context is locked, this function does nothing but log a warning.
    fn save_gas_remaining_before_subexecution(&self, gas_remaining: u64) {
        match self.context.try_lock() {
            Some(mut context) => {
                context.gas_remaining_before_subexecution = Some(gas_remaining);
            }
            None => {
                warn!("Context is locked, cannot save gas remaining before subexecution");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_models::address::Address;
    use massa_signature::KeyPair;

    // Tests the get_keys_wasmv1 interface method used by the updated get_keys abi.
    #[test]
    fn test_get_keys() {
        let sender_addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
        let interface = InterfaceImpl::new_default(sender_addr, None);

        interface
            .set_ds_value_wasmv1(b"k1", b"v1", Some(sender_addr.to_string()))
            .unwrap();
        interface
            .set_ds_value_wasmv1(b"k2", b"v2", Some(sender_addr.to_string()))
            .unwrap();
        interface
            .set_ds_value_wasmv1(b"l3", b"v3", Some(sender_addr.to_string()))
            .unwrap();

        let keys = interface.get_ds_keys_wasmv1(b"k", None).unwrap();

        assert_eq!(keys.len(), 2);
        assert!(keys.contains(b"k1".as_slice()));
        assert!(keys.contains(b"k2".as_slice()));
    }

    // Tests the get_op_keys_wasmv1 interface method used by the updated get_op_keys abi.
    #[test]
    fn test_get_op_keys() {
        let sender_addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());

        let mut operation_datastore = Datastore::new();
        operation_datastore.insert(b"k1".to_vec(), b"v1".to_vec());
        operation_datastore.insert(b"k2".to_vec(), b"v2".to_vec());
        operation_datastore.insert(b"l3".to_vec(), b"v3".to_vec());

        let interface = InterfaceImpl::new_default(sender_addr, Some(operation_datastore));

        let op_keys = interface.get_op_keys_wasmv1(b"k").unwrap();

        assert_eq!(op_keys.len(), 2);
        assert!(op_keys.contains(&b"k1".to_vec()));
        assert!(op_keys.contains(&b"k2".to_vec()));
    }

    #[test]
    fn test_native_amount() {
        let sender_addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
        let interface = InterfaceImpl::new_default(sender_addr, None);

        let amount1 = interface.native_amount_from_str_wasmv1("100").unwrap();
        let amount2 = interface.native_amount_from_str_wasmv1("100").unwrap();
        let amount3 = interface.native_amount_from_str_wasmv1("200").unwrap();

        let sum = interface
            .add_native_amount_wasmv1(&amount1, &amount2)
            .unwrap();

        assert_eq!(amount3, sum);
        println!(
            "sum: {}",
            interface.native_amount_to_string_wasmv1(&sum).unwrap()
        );
        assert_eq!(
            "200",
            interface.native_amount_to_string_wasmv1(&sum).unwrap()
        );

        let diff = interface.sub_native_amount_wasmv1(&sum, &amount2).unwrap();
        assert_eq!(amount1, diff);

        let amount4 = NativeAmount {
            mantissa: 1,
            scale: 9,
        };

        let is_valid = interface.check_native_amount_wasmv1(&amount4).unwrap();
        assert!(is_valid);

        let mul = interface
            .scalar_mul_native_amount_wasmv1(&amount1, 2)
            .unwrap();
        assert_eq!(mul, amount3);

        let (quotient, remainder) = interface
            .scalar_div_rem_native_amount_wasmv1(&amount1, 2)
            .unwrap();
        let quotient_res_50 = interface.native_amount_from_str_wasmv1("50").unwrap();
        let remainder_res_0 = interface.native_amount_from_str_wasmv1("0").unwrap();
        assert_eq!(quotient, quotient_res_50);
        assert_eq!(remainder, remainder_res_0);

        let (quotient, remainder) = interface
            .scalar_div_rem_native_amount_wasmv1(&amount1, 3)
            .unwrap();
        let verif_div = interface
            .scalar_mul_native_amount_wasmv1(&quotient, 3)
            .unwrap();
        let verif_dif = interface
            .add_native_amount_wasmv1(&verif_div, &remainder)
            .unwrap();
        assert_eq!(verif_dif, amount1);

        let amount5 = interface.native_amount_from_str_wasmv1("2").unwrap();
        let (quotient, remainder) = interface
            .div_rem_native_amount_wasmv1(&amount1, &amount5)
            .unwrap();
        assert_eq!(quotient, 50);
        assert_eq!(remainder, remainder_res_0);

        let amount6 = interface.native_amount_from_str_wasmv1("3").unwrap();
        let (quotient, remainder) = interface
            .div_rem_native_amount_wasmv1(&amount1, &amount6)
            .unwrap();
        let verif_div = interface
            .scalar_mul_native_amount_wasmv1(&amount6, quotient)
            .unwrap();
        let verif_dif = interface
            .add_native_amount_wasmv1(&verif_div, &remainder)
            .unwrap();
        assert_eq!(verif_dif, amount1);
    }

    #[test]
    fn test_base58_check_to_form() {
        let sender_addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
        let interface = InterfaceImpl::new_default(sender_addr, None);

        let data = "helloworld";
        let encoded = interface.bytes_to_base58_check_wasmv1(data.as_bytes());
        let decoded = interface.base58_check_to_bytes_wasmv1(&encoded).unwrap();

        assert_eq!(data.as_bytes(), decoded);
    }
    #[test]
    fn test_comparison_function() {
        let sender_addr = Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key());
        let interface = InterfaceImpl::new_default(sender_addr, None);

        // address
        let addr1 =
            Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key()).to_string();
        let addr2 =
            Address::from_public_key(&KeyPair::generate(0).unwrap().get_public_key()).to_string();

        let cmp_res = interface.compare_address_wasmv1(&addr1, &addr1).unwrap();
        println!("compare_address_wasmv1(: {}", cmp_res.as_str_name());
        assert_eq!(
            cmp_res,
            ComparisonResult::Equal,
            "  > Error: compare_address_wasmv1((addr1, addr1) should return EQUAL"
        );

        let cmp_res1 = interface.compare_address_wasmv1(&addr1, &addr2).unwrap();
        println!(
            "compare_address_wasmv1((addr1, addr2): {}",
            cmp_res1.as_str_name()
        );

        let cmp_res2 = interface.compare_address_wasmv1(&addr2, &addr1).unwrap();
        println!(
            "compare_address_wasmv1((addr2, addr1): {}",
            cmp_res2.as_str_name()
        );

        if cmp_res1 == ComparisonResult::Lower {
            assert_eq!(
                cmp_res2,
                ComparisonResult::Greater,
                "  > Error: compare_address_wasmv1((addr2, addr1) should return GREATER"
            );
        } else if cmp_res1 == ComparisonResult::Greater {
            assert_eq!(
                cmp_res2,
                ComparisonResult::Lower,
                "  > Error: compare_address_wasmv1((addr2, addr1) should return LOWER"
            );
        } else {
            assert_eq!(
                cmp_res1, cmp_res2,
                "  > Error: compare_address_wasmv1((addr2, addr1) should return EQUAL"
            );
        }

        //amount
        let amount1 = interface.native_amount_from_str_wasmv1("1").unwrap();
        let amount2 = interface.native_amount_from_str_wasmv1("2").unwrap();
        println!("do some compare with amount1 = 1, amount2 = 2");

        let cmp_res = interface
            .compare_native_amount_wasmv1(&amount1, &amount1)
            .unwrap();
        println!(
            "compare_native_amount_wasmv1(amount1, amount1): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Equal,
            "  > Error: compare_native_amount_wasmv1(amount1, amount1) should return EQUAL"
        );

        let cmp_res = interface
            .compare_native_amount_wasmv1(&amount1, &amount2)
            .unwrap();
        println!(
            "compare_native_amount_wasmv1(amount1, amount2): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Lower,
            "  > Error: compare_native_amount_wasmv1(amount1, amount2) should return LOWER"
        );

        let cmp_res = interface
            .compare_native_amount_wasmv1(&amount2, &amount1)
            .unwrap();
        println!(
            "compare_native_amount_wasmv1(amount2, amount1): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Greater,
            "  > Error: compare_native_amount_wasmv1(amount2, amount1) should return GREATER"
        );

        //time
        let time1 = massa_time_to_native_time(&MassaTime::from_millis(1));
        let time2 = massa_time_to_native_time(&MassaTime::from_millis(2));
        println!(
            "do some compare with time1 = {}, time2 = {}",
            time1.milliseconds, time2.milliseconds
        );

        let cmp_res = interface
            .compare_native_time_wasmv1(&time1, &time1)
            .unwrap();
        println!(
            "compare_native_time_wasmv1(time1, time1): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Equal,
            "  > Error:compare_native_time_wasmv1(time1, time1) should return EQUAL"
        );

        let cmp_res = interface
            .compare_native_time_wasmv1(&time1, &time2)
            .unwrap();
        println!(
            "compare_native_time_wasmv1(time1, time2): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Lower,
            "  > Error: compare_native_time_wasmv1(time1, time2) should return LOWER"
        );

        let cmp_res = interface
            .compare_native_time_wasmv1(&time2, &time1)
            .unwrap();
        println!(
            "compare_native_time_wasmv1(time2, time1): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Greater,
            "  > Error: compare_native_time_wasmv1(time2, time1) should return GREATER"
        );

        //pub_key
        let pub_key1 = KeyPair::generate(0).unwrap().get_public_key().to_string();
        let pub_key2 = KeyPair::generate(0).unwrap().get_public_key().to_string();

        println!(
            "do some compare with pub_key1 = {}, pub_key2 = {}",
            pub_key1, pub_key2
        );

        let cmp_res = interface
            .compare_pub_key_wasmv1(&pub_key1, &pub_key1)
            .unwrap();
        println!(
            "compare_pub_key_wasmv1(pub_key1, pub_key1): {}",
            cmp_res.as_str_name()
        );
        assert_eq!(
            cmp_res,
            ComparisonResult::Equal,
            "  > Error: compare_pub_key_wasmv1(pub_key1, pub_key1) should return EQUAL"
        );
        let cmp_res1 = interface
            .compare_pub_key_wasmv1(&pub_key1, &pub_key2)
            .unwrap();
        println!(
            "compare_pub_key_wasmv1(pub_key1, pub_key2): {}",
            cmp_res1.as_str_name()
        );
        let cmp_res2 = interface
            .compare_pub_key_wasmv1(&pub_key2, &pub_key1)
            .unwrap();
        println!(
            "compare_pub_key_wasmv1(pub_key2, pub_key1): {}",
            cmp_res2.as_str_name()
        );
        if cmp_res1 == ComparisonResult::Lower {
            assert_eq!(
                cmp_res2,
                ComparisonResult::Greater,
                "  > Error: compare_pub_key_wasmv1((pub_key2, pub_key1) should return GREATER"
            );
        } else if cmp_res1 == ComparisonResult::Greater {
            assert_eq!(
                cmp_res2,
                ComparisonResult::Lower,
                "  > Error: compare_pub_key_wasmv1((pub_key2, pub_key1) should return LOWER"
            );
        } else {
            assert_eq!(
                cmp_res1, cmp_res2,
                "  > Error: compare_pub_key_wasmv1((pub_key2, pub_key1) should return EQUAL"
            );
        }
    }
}

#[test]
fn test_evm_verify() {
    use hex_literal::hex;

    // signature info
    let address_ = hex!("807a7bb5193edf9898b9092c1597bb966fe52514");
    let message_ = b"test";
    let signature_ = hex!("d0d05c35080635b5e865006c6c4f5b5d457ec342564d8fc67ce40edc264ccdab3f2f366b5bd1e38582538fed7fa6282148e86af97970a10cb3302896f5d68ef51b");
    let private_key_ = hex!("ed6602758bdd68dc9df67a6936ed69807a74b8cc89bdc18f3939149d02db17f3");

    // build original public key
    let private_key = libsecp256k1::SecretKey::parse_slice(&private_key_).unwrap();
    let public_key = libsecp256k1::PublicKey::from_secret_key(&private_key);

    // build the message
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message_.len());
    let to_hash = [prefix.as_bytes(), message_].concat();
    let full_hash = sha3::Keccak256::digest(to_hash);
    let message = libsecp256k1::Message::parse_slice(&full_hash).unwrap();

    // parse the signature as being (r, s, v)
    // r is the R.x value of the signature's R point (32 bytes)
    // s is the signature proof for R.x (32 bytes)
    // v is a recovery parameter used to ease the signature verification (1 byte)
    let signature = libsecp256k1::Signature::parse_standard_slice(&signature_[..64]).unwrap();
    let recovery_id = libsecp256k1::RecoveryId::parse_rpc(signature_[64]).unwrap();

    // check 1
    // verify the signature
    assert!(libsecp256k1::verify(&message, &signature, &public_key));

    // check 2
    // recover the public key using v and match it with the derived one
    let recovered = libsecp256k1::recover(&message, &signature, &recovery_id).unwrap();
    assert_eq!(public_key, recovered);

    // check 3
    // sign the message and match it with the original signature
    let (second_signature, _) = libsecp256k1::sign(&message, &private_key);
    assert_eq!(signature, second_signature);

    // check 4
    // generate the address from the public key and match it with the original address
    // address is the last 20 bytes of the hash of the public key in raw format (64 bytes)
    let raw_public_key = public_key.serialize();
    let hash = sha3::Keccak256::digest(&raw_public_key[1..]).to_vec();
    let generated_address = &hash[12..];
    assert_eq!(generated_address, address_);
}
