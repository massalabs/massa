// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module represents the context in which the VM executes bytecode.
//! It provides information such as the current call stack.
//! It also maintains a "speculative" ledger state which is a virtual ledger
//! as seen after applying everything that happened so far in the context.
//! More generally, the context acts only on its own state
//! and does not write anything persistent to the consensus state.

use crate::active_history::HistorySearchResult;
use crate::speculative_async_pool::SpeculativeAsyncPool;
use crate::speculative_deferred_calls::SpeculativeDeferredCallRegistry;
use crate::speculative_executed_denunciations::SpeculativeExecutedDenunciations;
use crate::speculative_executed_ops::SpeculativeExecutedOps;
use crate::speculative_ledger::SpeculativeLedger;
use crate::{active_history::ActiveHistory, speculative_roll_state::SpeculativeRollState};
use massa_async_pool::{AsyncMessage, AsyncPoolChanges};
use massa_async_pool::{AsyncMessageId, AsyncMessageInfo};
use massa_deferred_calls::registry_changes::DeferredRegistryChanges;
use massa_deferred_calls::{DeferredCall, DeferredSlotCalls};
use massa_executed_ops::{ExecutedDenunciationsChanges, ExecutedOpsChanges};
use massa_execution_exports::{
    EventStore, ExecutedBlockInfo, ExecutionConfig, ExecutionError, ExecutionOutput,
    ExecutionStackElement,
};
use massa_final_state::{FinalStateController, StateChanges};
use massa_hash::Hash;
use massa_ledger_exports::{LedgerChanges, SetOrKeep};
use massa_models::address::ExecutionAddressCycleInfo;
use massa_models::block_id::BlockIdSerializer;
use massa_models::bytecode::Bytecode;
use massa_models::deferred_call_id::DeferredCallId;
use massa_models::denunciation::DenunciationIndex;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    operation::OperationId,
    output_event::{EventExecutionContext, SCOutputEvent},
    slot::Slot,
};
use massa_module_cache::controller::ModuleCache;
use massa_pos_exports::PoSChanges;
use massa_serialization::Serializer;
use massa_versioning::address_factory::{AddressArgs, AddressFactory};
use massa_versioning::versioning::MipStore;
use massa_versioning::versioning_factory::{FactoryStrategy, VersioningFactory};
use parking_lot::RwLock;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tracing::{debug, warn};

/// A snapshot taken from an `ExecutionContext` and that represents its current state.
/// The `ExecutionContext` state can then be restored later from this snapshot.
pub struct ExecutionContextSnapshot {
    /// speculative ledger changes caused so far in the context
    pub ledger_changes: LedgerChanges,

    /// speculative asynchronous pool messages emitted so far in the context
    pub async_pool_changes: AsyncPoolChanges,

    /// speculative deferred calls changes
    pub deferred_calls_changes: DeferredRegistryChanges,

    /// the associated message infos for the speculative async pool
    pub message_infos: BTreeMap<AsyncMessageId, AsyncMessageInfo>,

    /// speculative list of operations executed
    pub executed_ops: ExecutedOpsChanges,

    /// speculative list of executed denunciations
    pub executed_denunciations: ExecutedDenunciationsChanges,

    /// speculative roll state changes caused so far in the context
    pub pos_changes: PoSChanges,

    /// counter of newly created addresses so far at this slot during this execution
    pub created_addr_index: u64,

    /// counter of newly created events so far during this execution
    pub created_event_index: u64,

    /// counter of async messages emitted so far in this execution
    pub created_message_index: u64,

    /// address call stack, most recent is at the back
    pub stack: Vec<ExecutionStackElement>,

    /// keep the count of event emitted in the context
    pub event_count: usize,

    /// Unsafe random state
    pub unsafe_rng: Xoshiro256PlusPlus,

    /// The gas remaining before the last subexecution.
    /// so *excluding* the gas used by the last sc call.
    pub gas_remaining_before_subexecution: Option<u64>,
}

/// An execution context that needs to be initialized before executing bytecode,
/// passed to the VM to interact with during bytecode execution (through ABIs),
/// and read after execution to gather results.
pub struct ExecutionContext {
    /// configuration
    config: ExecutionConfig,

    /// speculative ledger state,
    /// as seen after everything that happened so far in the context
    #[cfg(all(
        not(feature = "gas_calibration"),
        not(feature = "benchmarking"),
        not(feature = "test-exports"),
        not(test)
    ))]
    speculative_ledger: SpeculativeLedger,
    #[cfg(any(
        feature = "gas_calibration",
        feature = "benchmarking",
        feature = "test-exports",
        test
    ))]
    pub(crate) speculative_ledger: SpeculativeLedger,

    /// speculative asynchronous pool state,
    /// as seen after everything that happened so far in the context
    speculative_async_pool: SpeculativeAsyncPool,

    /// speculative deferred calls state,
    speculative_deferred_calls: SpeculativeDeferredCallRegistry,

    /// speculative roll state,
    /// as seen after everything that happened so far in the context
    speculative_roll_state: SpeculativeRollState,

    /// speculative list of executed operations
    speculative_executed_ops: SpeculativeExecutedOps,

    /// speculative list of executed denunciations
    speculative_executed_denunciations: SpeculativeExecutedDenunciations,

    /// minimal balance allowed for the creator of the operation after its execution
    pub creator_min_balance: Option<Amount>,

    /// slot at which the execution happens
    pub slot: Slot,

    /// counter of newly created addresses so far during this execution
    pub created_addr_index: u64,

    /// counter of newly created events so far during this execution
    pub created_event_index: u64,

    /// counter of newly created messages so far during this execution
    pub created_message_index: u64,

    /// block Id, if one is present at the execution slot
    pub opt_block_id: Option<BlockId>,

    /// address call stack, most recent is at the back
    pub stack: Vec<ExecutionStackElement>,

    /// True if it's a read-only context
    pub read_only: bool,

    /// generated events during this execution, with multiple indexes
    pub events: EventStore,

    /// Unsafe random state (can be predicted and manipulated)
    pub unsafe_rng: Xoshiro256PlusPlus,

    /// Creator address. The bytecode of this address can't be modified
    pub creator_address: Option<Address>,

    /// operation id that originally caused this execution (if any)
    pub origin_operation_id: Option<OperationId>,

    /// Execution trail hash
    pub execution_trail_hash: Hash,

    /// cache of compiled runtime modules
    pub module_cache: Arc<RwLock<ModuleCache>>,

    /// Address factory
    pub address_factory: AddressFactory,

    /// The gas remaining before the last subexecution.
    /// so *excluding* the gas used by the last sc call.
    pub gas_remaining_before_subexecution: Option<u64>,
}

impl ExecutionContext {
    /// Create a new empty `ExecutionContext`
    /// This should only be used as a placeholder.
    /// Further initialization is required before running bytecode
    /// (see read-only and `active_slot` methods).
    ///
    /// # arguments
    /// * `final_state`: thread-safe access to the final state.
    /// Note that this will be used only for reading, never for writing
    ///
    /// # returns
    /// A new (empty) `ExecutionContext` instance
    pub(crate) fn new(
        config: ExecutionConfig,
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
        module_cache: Arc<RwLock<ModuleCache>>,
        mip_store: MipStore,
        execution_trail_hash: massa_hash::Hash,
    ) -> Self {
        ExecutionContext {
            speculative_ledger: SpeculativeLedger::new(
                final_state.clone(),
                active_history.clone(),
                config.max_datastore_key_length,
                config.max_bytecode_size,
                config.max_datastore_value_size,
                config.storage_costs_constants,
            ),
            speculative_async_pool: SpeculativeAsyncPool::new(
                final_state.clone(),
                active_history.clone(),
            ),
            speculative_deferred_calls: SpeculativeDeferredCallRegistry::new(
                final_state.clone(),
                active_history.clone(),
            ),
            speculative_roll_state: SpeculativeRollState::new(
                final_state.clone(),
                active_history.clone(),
            ),
            speculative_executed_ops: SpeculativeExecutedOps::new(
                final_state.clone(),
                active_history.clone(),
            ),
            speculative_executed_denunciations: SpeculativeExecutedDenunciations::new(
                final_state,
                active_history,
            ),
            creator_min_balance: Default::default(),
            slot: Slot::new(0, 0),
            created_addr_index: Default::default(),
            created_event_index: Default::default(),
            created_message_index: Default::default(),
            opt_block_id: Default::default(),
            stack: Default::default(),
            read_only: Default::default(),
            events: Default::default(),
            unsafe_rng: init_prng(&execution_trail_hash),
            creator_address: Default::default(),
            origin_operation_id: Default::default(),
            module_cache,
            config,
            address_factory: AddressFactory { mip_store },
            execution_trail_hash,
            gas_remaining_before_subexecution: None,
        }
    }

    /// Returns a snapshot containing the clone of the current execution state.
    /// Note that the snapshot does not include slot-level information such as the slot number or block ID.
    pub(crate) fn get_snapshot(&self) -> ExecutionContextSnapshot {
        let (async_pool_changes, message_infos) = self.speculative_async_pool.get_snapshot();
        ExecutionContextSnapshot {
            ledger_changes: self.speculative_ledger.get_snapshot(),
            async_pool_changes,
            deferred_calls_changes: self.speculative_deferred_calls.get_snapshot(),
            message_infos,
            pos_changes: self.speculative_roll_state.get_snapshot(),
            executed_ops: self.speculative_executed_ops.get_snapshot(),
            executed_denunciations: self.speculative_executed_denunciations.get_snapshot(),
            created_addr_index: self.created_addr_index,
            created_event_index: self.created_event_index,
            created_message_index: self.created_message_index,
            stack: self.stack.clone(),
            event_count: self.events.0.len(),
            unsafe_rng: self.unsafe_rng.clone(),
            gas_remaining_before_subexecution: self.gas_remaining_before_subexecution,
        }
    }

    /// Resets context to an existing snapshot.
    /// Optionally emits an error as an event after restoring the snapshot.
    /// Note that the snapshot does not include slot-level information such as the slot number or block ID.
    ///
    /// # Arguments
    /// * `snapshot`: a saved snapshot to be restored
    /// * `error`: an execution error to emit as an event conserved after snapshot reset.
    pub fn reset_to_snapshot(&mut self, snapshot: ExecutionContextSnapshot, error: ExecutionError) {
        // Reset context to snapshot.
        self.speculative_ledger
            .reset_to_snapshot(snapshot.ledger_changes);
        self.speculative_async_pool
            .reset_to_snapshot((snapshot.async_pool_changes, snapshot.message_infos));
        self.speculative_deferred_calls
            .reset_to_snapshot(snapshot.deferred_calls_changes);
        self.speculative_roll_state
            .reset_to_snapshot(snapshot.pos_changes);
        self.speculative_executed_ops
            .reset_to_snapshot(snapshot.executed_ops);
        self.speculative_executed_denunciations
            .reset_to_snapshot(snapshot.executed_denunciations);
        self.created_addr_index = snapshot.created_addr_index;
        self.created_event_index = snapshot.created_event_index;
        self.created_message_index = snapshot.created_message_index;
        self.stack = snapshot.stack;
        self.unsafe_rng = snapshot.unsafe_rng;
        self.gas_remaining_before_subexecution = snapshot.gas_remaining_before_subexecution;

        // For events, set snapshot delta to error events.
        for event in self.events.0.range_mut(snapshot.event_count..) {
            event.context.is_error = true;
        }

        // Emit the error event.
        // Note that the context event counter is properly handled by event_emit (see doc).
        self.event_emit(self.event_create(
            serde_json::json!({ "massa_execution_error": format!("{}", error) }).to_string(),
            true,
        ));
    }

    /// Create a new `ExecutionContext` for read-only execution
    /// This should be used before performing a read-only execution.
    ///
    /// # arguments
    /// * `slot`: slot at which the execution will happen
    /// * `req`: parameters of the read only execution
    /// * `final_state`: thread-safe access to the final state. Note that this will be used only for reading, never for writing
    ///
    /// # returns
    /// A `ExecutionContext` instance ready for a read-only execution
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn readonly(
        config: ExecutionConfig,
        slot: Slot,
        call_stack: Vec<ExecutionStackElement>,
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
        module_cache: Arc<RwLock<ModuleCache>>,
        mip_store: MipStore,
    ) -> Self {
        // Get the execution hash trail
        let prev_execution_trail_hash = active_history.read().get_execution_trail_hash();
        let prev_execution_trail_hash = match prev_execution_trail_hash {
            HistorySearchResult::Present(h) => h,
            _ => final_state.read().get_execution_trail_hash(),
        };
        let execution_trail_hash =
            generate_execution_trail_hash(&prev_execution_trail_hash, &slot, None, true);

        // return readonly context
        ExecutionContext {
            slot,
            stack: call_stack,
            read_only: true,
            ..ExecutionContext::new(
                config,
                final_state,
                active_history,
                module_cache,
                mip_store,
                execution_trail_hash,
            )
        }
    }

    /// This function takes a batch of asynchronous operations to execute, removing them from the speculative pool.
    ///
    /// # Arguments
    /// * `max_gas`: maximal amount of asynchronous gas available
    ///
    /// # Returns
    /// A vector of `(Option<Bytecode>, AsyncMessage)` pairs where:
    /// * `Option<Bytecode>` is the bytecode to execute (or `None` if not found)
    /// * `AsyncMessage` is the asynchronous message to execute
    pub(crate) fn take_async_batch(
        &mut self,
        max_gas: u64,
        async_msg_cst_gas_cost: u64,
    ) -> Vec<(Option<Bytecode>, AsyncMessage)> {
        self.speculative_async_pool
            .take_batch_to_execute(self.slot, max_gas, async_msg_cst_gas_cost)
            .into_iter()
            .map(|(_id, msg)| (self.get_bytecode(&msg.destination), msg))
            .collect()
    }

    /// Create a new `ExecutionContext` for executing an active slot.
    /// This should be used before performing any executions at that slot.
    ///
    /// # arguments
    /// * `slot`: slot at which the execution will happen
    /// * `opt_block_id`: optional ID of the block at that slot
    /// * `final_state`: thread-safe access to the final state. Note that this will be used only for reading, never for writing
    ///
    /// # returns
    /// A `ExecutionContext` instance
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn active_slot(
        config: ExecutionConfig,
        slot: Slot,
        opt_block_id: Option<BlockId>,
        final_state: Arc<RwLock<dyn FinalStateController>>,
        active_history: Arc<RwLock<ActiveHistory>>,
        module_cache: Arc<RwLock<ModuleCache>>,
        mip_store: MipStore,
    ) -> Self {
        // Get the execution hash trail
        let prev_execution_trail_hash = active_history.read().get_execution_trail_hash();
        let prev_execution_trail_hash = match prev_execution_trail_hash {
            HistorySearchResult::Present(h) => h,
            _ => final_state.read().get_execution_trail_hash(),
        };
        let execution_trail_hash = generate_execution_trail_hash(
            &prev_execution_trail_hash,
            &slot,
            opt_block_id.as_ref(),
            false,
        );

        // return active slot execution context
        ExecutionContext {
            slot,
            opt_block_id,
            ..ExecutionContext::new(
                config,
                final_state,
                active_history,
                module_cache,
                mip_store,
                execution_trail_hash,
            )
        }
    }

    /// Gets the address at the top of the call stack, if any
    pub fn get_current_address(&self) -> Result<Address, ExecutionError> {
        match self.stack.last() {
            Some(addr) => Ok(addr.address),
            _ => Err(ExecutionError::RuntimeError(
                "failed to read current address: call stack empty".into(),
            )),
        }
    }

    /// Gets the current list of owned addresses (top of the stack)
    /// Ordering is conserved for determinism
    pub fn get_current_owned_addresses(&self) -> Result<Vec<Address>, ExecutionError> {
        match self.stack.last() {
            Some(v) => Ok(v.owned_addresses.clone()),
            None => Err(ExecutionError::RuntimeError(
                "failed to read current owned addresses list: call stack empty".into(),
            )),
        }
    }

    /// Gets the current call coins
    pub fn get_current_call_coins(&self) -> Result<Amount, ExecutionError> {
        match self.stack.last() {
            Some(v) => Ok(v.coins),
            None => Err(ExecutionError::RuntimeError(
                "failed to read current call coins: call stack empty".into(),
            )),
        }
    }

    /// Gets the addresses from the call stack (last = top of the stack)
    pub fn get_call_stack(&self) -> Vec<Address> {
        self.stack.iter().map(|v| v.address).collect()
    }

    /// Checks whether the context currently grants write access to a given address
    pub fn has_write_rights_on(&self, addr: &Address) -> bool {
        self.stack
            .last()
            .map_or(false, |v| v.owned_addresses.contains(addr))
    }

    /// Creates a new smart contract address with initial bytecode, and returns this address
    pub fn create_new_sc_address(&mut self, bytecode: Bytecode) -> Result<Address, ExecutionError> {
        // deterministically generate a new unique smart contract address
        let slot_timestamp = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            self.slot,
        )
        .expect("could not compute current slot timestamp");

        // Loop over nonces until we find an address that doesn't exist in the speculative ledger.
        // Note that this loop is here for robustness, and should not be looping because
        // even through the SC addresses are predictable, nobody can create them beforehand because:
        // - their category is "SC" and not "USER" so they can't be derived from a public key
        // - sending tokens to the target SC address to create it by funding is not allowed because transactions towards SC addresses are not allowed
        let mut nonce = 0u64;
        let address = loop {
            // get a deterministic seed hash
            let hash = massa_hash::Hash::compute_from_tuple(&[
                "SC_ADDRESS".as_bytes(),
                self.execution_trail_hash.to_bytes(),
                &self.created_addr_index.to_be_bytes(),
                &nonce.to_be_bytes(),
            ]);

            // deduce the address
            let addr = self.address_factory.create(
                &AddressArgs::SC { hash },
                FactoryStrategy::At(slot_timestamp),
            )?;

            // check if this address already exists in the speculative ledger
            if !self.speculative_ledger.entry_exists(&addr) {
                // if not, we can use it
                break addr;
            }

            // otherwise, increment the nonce to get a new hash and try again
            nonce = nonce.checked_add(1).ok_or_else(|| {
                ExecutionError::RuntimeError("nonce overflow when creating SC address".into())
            })?;
        };

        // add this address with its bytecode to the speculative ledger
        self.speculative_ledger.create_new_sc_address(
            self.get_current_address()?,
            address,
            bytecode,
        )?;

        // add the address to owned addresses
        // so that the current call has write access to it
        // from now and for its whole duration,
        // in order to allow initializing newly created ledger entries.
        match self.stack.last_mut() {
            Some(v) => {
                v.owned_addresses.push(address);
            }
            None => {
                return Err(ExecutionError::RuntimeError(
                    "owned addresses not found in context stack".into(),
                ));
            }
        };

        // increment the address creation counter at this slot
        self.created_addr_index += 1;

        Ok(address)
    }

    /// gets the bytecode of an address if it exists in the speculative ledger, or returns None
    pub fn get_bytecode(&self, address: &Address) -> Option<Bytecode> {
        self.speculative_ledger.get_bytecode(address)
    }

    /// gets the datastore keys of an address if it exists in the speculative ledger, or returns None
    pub fn get_keys(&self, address: &Address, prefix: &[u8]) -> Option<BTreeSet<Vec<u8>>> {
        self.speculative_ledger.get_keys(address, prefix)
    }

    /// gets the data from a datastore entry of an address if it exists in the speculative ledger, or returns None
    pub fn get_data_entry(&self, address: &Address, key: &[u8]) -> Option<Vec<u8>> {
        self.speculative_ledger.get_data_entry(address, key)
    }

    /// checks if a datastore entry exists in the speculative ledger
    pub fn has_data_entry(&self, address: &Address, key: &[u8]) -> bool {
        self.speculative_ledger.has_data_entry(address, key)
    }

    /// gets the effective balance of an address
    pub fn get_balance(&self, address: &Address) -> Option<Amount> {
        self.speculative_ledger.get_balance(address)
    }

    /// Sets a datastore entry for an address in the speculative ledger.
    /// Fail if the address is absent from the ledger.
    /// The datastore entry is created if it is absent for that address.
    ///
    /// # Arguments
    /// * address: the address of the ledger entry
    /// * key: the datastore key
    /// * data: the data to insert
    pub fn set_data_entry(
        &mut self,
        address: &Address,
        key: Vec<u8>,
        data: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check access right
        if !self.has_write_rights_on(address) {
            return Err(ExecutionError::RuntimeError(format!(
                "writing in the datastore of address {} is not allowed in this context",
                address
            )));
        }

        // set data entry
        self.speculative_ledger
            .set_data_entry(&self.get_current_address()?, address, key, data)
    }

    /// Appends data to a datastore entry for an address in the speculative ledger.
    /// Fail if the address is absent from the ledger.
    /// Fails if the datastore entry is absent for that address.
    ///
    /// # Arguments
    /// * address: the address of the ledger entry
    /// * key: the datastore key
    /// * data: the data to append
    pub fn append_data_entry(
        &mut self,
        address: &Address,
        key: Vec<u8>,
        data: Vec<u8>,
    ) -> Result<(), ExecutionError> {
        // check access right
        if !self.has_write_rights_on(address) {
            return Err(ExecutionError::RuntimeError(format!(
                "appending to the datastore of address {} is not allowed in this context",
                address
            )));
        }

        // get current data entry
        let mut res_data = self
            .speculative_ledger
            .get_data_entry(address, &key)
            .ok_or_else(|| {
                ExecutionError::RuntimeError(format!(
                    "appending to the datastore of address {} failed: entry {:?} not found",
                    address, key
                ))
            })?;

        // append data
        res_data.extend(data);

        // set data entry
        self.speculative_ledger
            .set_data_entry(&self.get_current_address()?, address, key, res_data)
    }

    /// Deletes a datastore entry for an address.
    /// Fails if the address or the entry does not exist or if write access rights are missing.
    ///
    /// # Arguments
    /// * address: the address of the ledger entry
    /// * key: the datastore key
    pub fn delete_data_entry(
        &mut self,
        address: &Address,
        key: &[u8],
    ) -> Result<(), ExecutionError> {
        // check access right
        if !self.has_write_rights_on(address) {
            return Err(ExecutionError::RuntimeError(format!(
                "deleting from the datastore of address {} is not allowed in this context",
                address
            )));
        }

        // delete entry
        self.speculative_ledger
            .delete_data_entry(&self.get_current_address()?, address, key)
    }

    /// Transfers coins from one address to another.
    /// No changes are retained in case of failure.
    /// Spending is only allowed from existing addresses we have write access on
    ///
    /// # Arguments
    /// * `from_addr`: optional spending address (use None for pure coin creation)
    /// * `to_addr`: optional crediting address (use None for pure coin destruction)
    /// * `amount`: amount of coins to transfer
    /// * `check_rights`: check that the sender has the right to spend the coins according to the call stack
    pub fn transfer_coins(
        &mut self,
        from_addr: Option<Address>,
        to_addr: Option<Address>,
        amount: Amount,
        check_rights: bool,
    ) -> Result<(), ExecutionError> {
        if let Some(from_addr) = &from_addr {
            // check access rights
            if check_rights {
                // ensure we can't spend from an address on which we have no write access
                if !self.has_write_rights_on(from_addr) {
                    return Err(ExecutionError::RuntimeError(format!(
                        "spending from address {} is not allowed in this context",
                        from_addr
                    )));
                }

                // ensure we can't transfer towards SC addresses on which we have no write access
                if let Some(to_addr) = &to_addr {
                    if matches!(to_addr, Address::SC(..)) && !self.has_write_rights_on(to_addr) {
                        return Err(ExecutionError::RuntimeError(format!(
                            "crediting SC address {} is not allowed without write access to it",
                            to_addr
                        )));
                    }
                }
            }
        }

        // do the transfer
        self.speculative_ledger
            .transfer_coins(from_addr, to_addr, amount)
    }

    /// Add a new asynchronous message to speculative pool
    ///
    /// # Arguments
    /// * `msg`: asynchronous message to add
    pub fn push_new_message(&mut self, msg: AsyncMessage) {
        self.speculative_async_pool.push_new_message(msg);
    }

    /// Cancels an asynchronous message, reimbursing `msg.coins` to the sender
    ///
    /// # Arguments
    /// * `msg`: the asynchronous message to cancel
    pub fn cancel_async_message(
        &mut self,
        msg: &AsyncMessage,
    ) -> Option<(Address, Result<Amount, String>)> {
        #[allow(unused_assignments, unused_mut)]
        let mut result = None;
        let transfer_result = self.transfer_coins(None, Some(msg.sender), msg.coins, false);
        if let Err(e) = transfer_result.as_ref() {
            debug!(
                "async message cancel: reimbursement of {} failed: {}",
                msg.sender, e
            );
        }

        #[cfg(feature = "execution-info")]
        if let Err(e) = transfer_result {
            result = Some((msg.sender, Err(e.to_string())))
        } else {
            result = Some((msg.sender, Ok(msg.coins)));
        }

        result
    }

    /// Add `roll_count` rolls to the buyer address.
    /// Validity checks must be performed _outside_ of this function.
    ///
    /// # Arguments
    /// * `buyer_addr`: address that will receive the rolls
    /// * `roll_count`: number of rolls it will receive
    pub fn add_rolls(&mut self, buyer_addr: &Address, roll_count: u64) {
        self.speculative_roll_state
            .add_rolls(buyer_addr, roll_count);
    }

    /// Try to sell `roll_count` rolls from the seller address.
    ///
    /// # Arguments
    /// * `seller_addr`: address to sell the rolls from
    /// * `roll_count`: number of rolls to sell
    pub fn try_sell_rolls(
        &mut self,
        seller_addr: &Address,
        roll_count: u64,
    ) -> Result<(), ExecutionError> {
        self.speculative_roll_state.try_sell_rolls(
            seller_addr,
            self.slot,
            roll_count,
            self.config.periods_per_cycle,
            self.config.thread_count,
            self.config.roll_price,
        )
    }

    /// Try to slash `roll_count` rolls from the denounced address. If not enough rolls,
    /// slash the available amount and return the result
    ///
    /// # Arguments
    /// * `denounced_addr`: address to sell the rolls from
    /// * `roll_count`: number of rolls to slash
    pub fn try_slash_rolls(
        &mut self,
        denounced_addr: &Address,
        roll_count: u64,
    ) -> Result<Amount, ExecutionError> {
        // try to slash as many roll as available
        let slashed_rolls = self
            .speculative_roll_state
            .try_slash_rolls(denounced_addr, roll_count);

        // convert slashed rolls to coins (as deferred credits => coins)
        let mut slashed_coins = self
            .config
            .roll_price
            .checked_mul_u64(slashed_rolls.unwrap_or_default())
            .ok_or_else(|| {
                ExecutionError::RuntimeError(format!(
                    "Cannot multiply roll price by {}",
                    roll_count
                ))
            })?;

        // what remains to slash (then will try to slash as many deferred credits as avail/what remains to be slashed)
        let amount_remaining_to_slash = self
            .config
            .roll_price
            .checked_mul_u64(roll_count)
            .ok_or_else(|| {
                ExecutionError::RuntimeError(format!(
                    "Cannot multiply roll price by {}",
                    roll_count
                ))
            })?
            .saturating_sub(slashed_coins);

        if amount_remaining_to_slash > Amount::zero() {
            // There is still an amount to slash for this denunciation so we need to slash
            // in deferred credits
            let slashed_coins_in_deferred_credits = self
                .speculative_roll_state
                .try_slash_deferred_credits(&self.slot, denounced_addr, &amount_remaining_to_slash);

            slashed_coins = slashed_coins.saturating_add(slashed_coins_in_deferred_credits);
            let amount_remaining_to_slash_2 =
                slashed_coins.saturating_sub(slashed_coins_in_deferred_credits);
            if amount_remaining_to_slash_2 > Amount::zero() {
                // Use saturating_mul_u64 to avoid an error (for just a warn!(..))
                warn!("Slashed {} coins (by selling rolls) and {} coins from deferred credits of address: {} but cumulative amount is lower than expected: {} coins",
                    slashed_coins, slashed_coins_in_deferred_credits, denounced_addr,
                    self.config.roll_price.saturating_mul_u64(roll_count)
                );
            }
        }

        Ok(slashed_coins)
    }

    /// Update production statistics of an address.
    ///
    /// # Arguments
    /// * `creator`: the supposed creator
    /// * `slot`: current slot
    /// * `block_id`: id of the block (if some)
    pub fn update_production_stats(
        &mut self,
        creator: &Address,
        slot: Slot,
        block_id: Option<BlockId>,
    ) {
        self.speculative_roll_state
            .update_production_stats(creator, slot, block_id);
    }

    /// Execute the deferred credits of `slot`.
    ///
    /// # Arguments
    /// * `slot`: associated slot of the deferred credits to be executed
    pub fn execute_deferred_credits(
        &mut self,
        slot: &Slot,
    ) -> Vec<(Address, Result<Amount, String>)> {
        #[allow(unused_mut)]
        let mut result = vec![];

        for (_slot, map) in self
            .speculative_roll_state
            .take_unexecuted_deferred_credits(slot)
            .credits
        {
            for (address, amount) in map {
                let transfer_result = self.transfer_coins(None, Some(address), amount, false);

                if let Err(e) = transfer_result.as_ref() {
                    debug!(
                        "could not credit {} deferred coins to {} at slot {}: {}",
                        amount, address, slot, e
                    );
                }

                #[cfg(feature = "execution-info")]
                if let Err(e) = transfer_result {
                    result.push((address, Err(e.to_string())));
                } else {
                    result.push((address, Ok(amount)));
                }
            }
        }

        result
    }

    /// Finishes a slot and generates the execution output.
    /// Settles emitted asynchronous messages, reimburse the senders of deleted messages.
    /// Moves the output of the execution out of the context,
    /// resetting some context fields in the process.
    ///
    /// This is used to get the output of an execution before discarding the context.
    /// Note that we are not taking self by value to consume it because the context is shared.
    pub fn settle_slot(&mut self, block_info: Option<ExecutedBlockInfo>) -> ExecutionOutput {
        let slot = self.slot;

        // execute the deferred credits coming from roll sells
        let deferred_credits_transfers = self.execute_deferred_credits(&slot);

        // take the ledger changes first as they are needed for async messages and cache
        let ledger_changes = self.speculative_ledger.take();

        // settle emitted async messages and reimburse the senders of deleted messages
        let deleted_messages = self
            .speculative_async_pool
            .settle_slot(&slot, &ledger_changes);

        let mut cancel_async_message_transfers = vec![];
        for (_msg_id, msg) in deleted_messages {
            if let Some(t) = self.cancel_async_message(&msg) {
                cancel_async_message_transfers.push(t)
            }
        }

        // update module cache
        let bc_updates = ledger_changes.get_bytecode_updates();
        {
            let mut cache_write_lock = self.module_cache.write();
            for bytecode in bc_updates {
                cache_write_lock.save_module(&bytecode.0);
            }
        }

        // if the current slot is last in cycle check the production stats and act accordingly
        let auto_sell_rolls = if self
            .slot
            .is_last_of_cycle(self.config.periods_per_cycle, self.config.thread_count)
        {
            self.speculative_roll_state.settle_production_stats(
                &slot,
                self.config.periods_per_cycle,
                self.config.thread_count,
                self.config.roll_price,
                self.config.max_miss_ratio,
            )
        } else {
            vec![]
        };

        // generate the execution output
        let state_changes = StateChanges {
            ledger_changes,
            async_pool_changes: self.speculative_async_pool.take(),
            deferred_call_changes: self.speculative_deferred_calls.take(),
            pos_changes: self.speculative_roll_state.take(),
            executed_ops_changes: self.speculative_executed_ops.take(),
            executed_denunciations_changes: self.speculative_executed_denunciations.take(),
            execution_trail_hash_change: SetOrKeep::Set(self.execution_trail_hash),
        };

        std::mem::take(&mut self.opt_block_id);
        ExecutionOutput {
            slot,
            block_info,
            state_changes,
            events: std::mem::take(&mut self.events),
            #[cfg(feature = "execution-trace")]
            slot_trace: None,
            #[cfg(feature = "dump-block")]
            storage: None,
            deferred_credits_execution: deferred_credits_transfers,
            cancel_async_message_execution: cancel_async_message_transfers,
            auto_sell_execution: auto_sell_rolls,
        }
    }

    /// Sets a bytecode for an address in the speculative ledger.
    /// Fail if the address is absent from the ledger.
    ///
    /// # Arguments
    /// * address: the address of the ledger entry
    /// * data: the bytecode to set
    pub fn set_bytecode(
        &mut self,
        address: &Address,
        bytecode: Bytecode,
    ) -> Result<(), ExecutionError> {
        // check access right
        if !self.has_write_rights_on(address) {
            return Err(ExecutionError::RuntimeError(format!(
                "setting the bytecode of address {} is not allowed in this context",
                address
            )));
        }

        // Do not allow user addresses to store bytecode.
        // See: https://github.com/massalabs/massa/discussions/2952
        if let Address::User(_) = address {
            return Err(ExecutionError::RuntimeError(format!(
                "can't set the bytecode of address {} because this is not a smart contract address",
                address
            )));
        }

        // set data entry
        self.speculative_ledger
            .set_bytecode(&self.get_current_address()?, address, bytecode)
    }

    /// Creates a new event but does not emit it.
    /// Note that this does not increment the context event counter.
    ///
    /// # Arguments:
    /// data: the string data that is the payload of the event
    pub fn event_create(&self, data: String, is_error: bool) -> SCOutputEvent {
        // Gather contextual information from the execution context
        let context = EventExecutionContext {
            slot: self.slot,
            block: self.opt_block_id,
            call_stack: self.stack.iter().map(|e| e.address).collect(),
            read_only: self.read_only,
            index_in_slot: self.created_event_index,
            origin_operation_id: self.origin_operation_id,
            is_final: false,
            is_error,
        };

        // Return the event
        SCOutputEvent { context, data }
    }

    /// Emits a previously created event.
    /// Overrides the event's index with the current event counter value, and increments the event counter.
    pub fn event_emit(&mut self, mut event: SCOutputEvent) {
        // Set the event index
        event.context.index_in_slot = self.created_event_index;

        // Increment the event counter fot this slot
        self.created_event_index += 1;

        // Add the event to the context store
        self.events.push(event);
    }

    /// Check if an operation was previously executed (to prevent reuse)
    pub fn is_op_executed(&self, op_id: &OperationId) -> bool {
        self.speculative_executed_ops.is_op_executed(op_id)
    }

    /// Check if a denunciation was previously executed (to prevent reuse)
    pub fn is_denunciation_executed(&self, de_idx: &DenunciationIndex) -> bool {
        self.speculative_executed_denunciations
            .is_denunciation_executed(de_idx)
    }

    /// Insert an executed operation.
    /// Does not check for reuse, please use `is_op_executed` before.
    ///
    /// # Arguments
    /// * `op_id`: operation ID
    /// * `op_exec_status` : the status of the execution of the operation (true: success, false: failed).
    /// * `op_valid_until_slot`: slot until which the operation remains valid (included)
    pub fn insert_executed_op(
        &mut self,
        op_id: OperationId,
        op_exec_status: bool,
        op_valid_until_slot: Slot,
    ) {
        self.speculative_executed_ops
            .insert_executed_op(op_id, op_exec_status, op_valid_until_slot)
    }

    /// Insert a executed denunciation.
    ///
    pub fn insert_executed_denunciation(&mut self, denunciation_idx: &DenunciationIndex) {
        self.speculative_executed_denunciations
            .insert_executed_denunciation(*denunciation_idx);
    }

    /// gets the cycle information for an address
    pub fn get_address_cycle_infos(
        &self,
        address: &Address,
        periods_per_cycle: u64,
    ) -> Vec<ExecutionAddressCycleInfo> {
        self.speculative_roll_state
            .get_address_cycle_infos(address, periods_per_cycle, self.slot)
    }

    /// Get future deferred credits of an address
    /// With optionally a limit slot (excluded)
    pub fn get_address_future_deferred_credits(
        &self,
        address: &Address,
        thread_count: u8,
        max_slot: std::ops::Bound<Slot>,
    ) -> BTreeMap<Slot, Amount> {
        let min_slot = self
            .slot
            .get_next_slot(thread_count)
            .expect("unexpected slot overflow in context.get_addresses_deferred_credits");
        self.speculative_roll_state
            .get_address_deferred_credits(address, (std::ops::Bound::Included(min_slot), max_slot))
    }

    /// in case of
    ///
    /// async_msg, call OP, call SC to SC, read only call
    ///
    /// check if the given address is a smart contract address and if it exists
    /// returns an error instead
    pub fn check_target_sc_address(
        &self,
        target_sc_address: Address,
    ) -> Result<(), ExecutionError> {
        match target_sc_address {
            Address::SC(..) => {
                // if the target address does not exist: fail
                if !self.speculative_ledger.entry_exists(&target_sc_address) {
                    return Err(ExecutionError::RuntimeError(format!(
                        "The called smart contract address {} does not exist",
                        target_sc_address
                    )));
                }
                Ok(())
            }
            // if the target address is not SC: fail
            _ => Err(ExecutionError::RuntimeError(format!(
                "The called address {} is not a smart contract address",
                target_sc_address
            ))),
        }
    }

    pub fn deferred_calls_get_slot_booked_gas(&self, slot: &Slot) -> u64 {
        self.speculative_deferred_calls.get_slot_gas(slot)
    }

    pub fn deferred_calls_advance_slot(
        &mut self,
        current_slot: Slot,
        async_call_max_booking_slots: u64,
        thread_count: u8,
    ) -> DeferredSlotCalls {
        self.speculative_deferred_calls.advance_slot(
            current_slot,
            async_call_max_booking_slots,
            thread_count,
        )
    }

    /// Get the price it would cost to reserve "gas" at target slot "slot".
    pub fn deferred_calls_compute_call_fee(
        &self,
        target_slot: Slot,
        max_gas: u64,
        thread_count: u8,
        async_call_max_booking_slots: u64,
        max_async_gas: u64,
        global_overbooking_penalty: Amount,
        slot_overbooking_penalty: Amount,
        current_slot: Slot,
    ) -> Result<Amount, ExecutionError> {
        self.speculative_deferred_calls.compute_call_fee(
            target_slot,
            max_gas,
            thread_count,
            async_call_max_booking_slots,
            max_async_gas,
            global_overbooking_penalty,
            slot_overbooking_penalty,
            current_slot,
        )
    }

    pub fn deferred_call_register(
        &mut self,
        call: DeferredCall,
    ) -> Result<DeferredCallId, ExecutionError> {
        self.speculative_deferred_calls
            .register_call(call, self.execution_trail_hash)
    }

    pub fn deferred_call_exist(&self, call_id: &DeferredCallId) -> bool {
        self.speculative_deferred_calls.get_call(call_id).is_some()
    }

    /// when a deferred call execution fail we need to refund the coins to the caller
    pub fn deferred_call_fail_exec(
        &mut self,
        call: &DeferredCall,
    ) -> Option<(Address, Result<Amount, String>)> {
        #[allow(unused_assignments, unused_mut)]
        let mut result = None;

        let transfer_result =
            self.transfer_coins(None, Some(call.sender_address), call.coins, false);
        if let Err(e) = transfer_result.as_ref() {
            debug!(
                "deferred call cancel: reimbursement of {} failed: {}",
                call.sender_address, e
            );
        }

        #[cfg(feature = "execution-info")]
        if let Err(e) = transfer_result {
            result = Some((call.sender_address, Err(e.to_string())))
        } else {
            result = Some((call.sender_address, Ok(call.coins)));
        }

        result
    }

    pub fn deferred_call_delete(&mut self, call_id: &DeferredCallId, slot: Slot) {
        self.speculative_deferred_calls.delete_call(call_id, slot);
    }

    pub fn deferred_call_cancel(
        &mut self,
        call_id: &DeferredCallId,
        caller_address: Address,
    ) -> Result<(), ExecutionError> {
        match self.speculative_deferred_calls.get_call(call_id) {
            Some(call) => {
                // check that the caller is the one who registered the deferred call
                if call.sender_address != caller_address {
                    return Err(ExecutionError::DeferredCallsError(format!(
                        "only the caller {} can cancel the deferred call",
                        call.sender_address
                    )));
                }

                let (address, amount) = self.speculative_deferred_calls.cancel_call(call_id)?;

                // refund the coins to the caller
                let transfer_result = self.transfer_coins(None, Some(address), amount, false);
                if let Err(e) = transfer_result.as_ref() {
                    debug!(
                        "deferred call cancel: reimbursement of {} failed: {}",
                        address, e
                    );
                }

                Ok(())
            }
            _ => Err(ExecutionError::DeferredCallsError(format!(
                "deferred call {} does not exist",
                call_id
            )))?,
        }
    }
}

/// Generate the execution trail hash
fn generate_execution_trail_hash(
    previous_execution_trail_hash: &massa_hash::Hash,
    slot: &Slot,
    opt_block_id: Option<&BlockId>,
    read_only: bool,
) -> massa_hash::Hash {
    match opt_block_id {
        None => massa_hash::Hash::compute_from_tuple(&[
            previous_execution_trail_hash.to_bytes(),
            &slot.to_bytes_key(),
            &[if read_only { 1u8 } else { 0u8 }, 0u8],
        ]),
        Some(block_id) => {
            let mut bytes = Vec::new();
            let block_id_serializer = BlockIdSerializer::new();
            block_id_serializer.serialize(block_id, &mut bytes).unwrap();
            massa_hash::Hash::compute_from_tuple(&[
                previous_execution_trail_hash.to_bytes(),
                &slot.to_bytes_key(),
                &[if read_only { 1u8 } else { 0u8 }, 1u8],
                &bytes,
            ])
        }
    }
}

/// Initializes and seeds the PRNG with the given execution trail hash.
fn init_prng(execution_trail_hash: &massa_hash::Hash) -> Xoshiro256PlusPlus {
    // Deterministically seed the unsafe RNG to allow the bytecode to use it.
    // Note that consecutive read-only calls for the same slot will get the same random seed.
    let seed = massa_hash::Hash::compute_from_tuple(&[
        "PRNG_SEED".as_bytes(),
        execution_trail_hash.to_bytes(),
    ])
    .into_bytes();

    // We use Xoshiro256PlusPlus because it is very fast,
    // has a period long enough to ensure no repetitions will ever happen,
    // of decent quality (given the unsafe constraints)
    // but not cryptographically secure (and that's ok because the internal state is exposed anyway)
    Xoshiro256PlusPlus::from_seed(seed)
}
