// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module represents the context in which the VM executes bytecode.
//! It provides information such as the current call stack.
//! It also maintians a "speculative" ledger state which is a virtual ledger
//! as seen after applying everything that happened so far in the context.
//! More generally, the context acts only on its own state
//! and does not write anything persistent to the conensus state.

use crate::speculative_ledger::SpeculativeLedger;
use massa_execution_exports::{
    EventStore, ExecutionError, ExecutionOutput, ExecutionStackElement, ReadOnlyExecutionRequest,
};
use massa_hash::hash::Hash;
use massa_ledger::{FinalLedger, LedgerChanges};
use massa_models::{Address, Amount, BlockId, OperationId, Slot};
use parking_lot::RwLock;
use rand::SeedableRng;
use rand_xoshiro::Xoshiro256PlusPlus;
use std::sync::Arc;

/// A snapshot taken from an ExecutionContext and that represents its current state.
/// The ExecutionContext state can then be restored later from this snapshot.
pub(crate) struct ExecutionContextSnapshot {
    // speculative ledger changes caused so far in the context
    pub ledger_changes: LedgerChanges,

    /// counter of newly created addresses so far at this slot during this execution
    pub created_addr_index: u64,

    /// counter of newly created events so far during this execution
    pub created_event_index: u64,

    /// address call stack, most recent is at the back
    pub stack: Vec<ExecutionStackElement>,

    /// generated events during this execution, with multiple indexes
    pub events: EventStore,

    /// Unsafe RNG state
    pub unsafe_rng: Xoshiro256PlusPlus,
}

/// An execution context that needs to be initialized before executing bytecode,
/// passed to the VM to interact with during bytecode execution (through ABIs),
/// and read after execution to gather results.
pub(crate) struct ExecutionContext {
    /// speculative ledger state,
    /// as seen after everything that happened so far in the context
    speculative_ledger: SpeculativeLedger,

    /// max gas for this execution
    pub max_gas: u64,

    /// gas price of the execution
    pub gas_price: Amount,

    /// slot at which the execution happens
    pub slot: Slot,

    /// counter of newly created addresses so far during this execution
    pub created_addr_index: u64,

    /// counter of newly created events so far during this execution
    pub created_event_index: u64,

    /// block ID, if one is present at the execution slot
    pub opt_block_id: Option<BlockId>,

    /// address call stack, most recent is at the back
    pub stack: Vec<ExecutionStackElement>,

    /// True if it's a read-only context
    pub read_only: bool,

    /// generated events during this execution, with multiple indexes
    pub events: EventStore,

    /// Unsafe RNG state (can be predicted and manipulated)
    pub unsafe_rng: Xoshiro256PlusPlus,

    /// operation id that originally caused this execution (if any)
    pub origin_operation_id: Option<OperationId>,
}

impl ExecutionContext {
    /// Create a new empty ExecutionContext
    /// This should only be used as a placeholder.
    /// Further initialization is required before running bytecode
    /// (see readonly and active_slot methods).
    ///
    /// # arguments
    /// * final_ledger: thread-safe access to the final ledger. Note that this will be used only for reading, never for writing
    /// * previous_changes: list of ledger changes that happened since the final ledger state and before the current execution
    ///
    /// # returns
    /// A new (empty) ExecutionContext instance
    pub(crate) fn new(
        final_ledger: Arc<RwLock<FinalLedger>>,
        previous_changes: LedgerChanges,
    ) -> Self {
        ExecutionContext {
            speculative_ledger: SpeculativeLedger::new(final_ledger, previous_changes),
            max_gas: Default::default(),
            gas_price: Default::default(),
            slot: Slot::new(0, 0),
            created_addr_index: Default::default(),
            created_event_index: Default::default(),
            opt_block_id: Default::default(),
            stack: Default::default(),
            read_only: Default::default(),
            events: Default::default(),
            unsafe_rng: Xoshiro256PlusPlus::from_seed([0u8; 32]),
            origin_operation_id: Default::default(),
        }
    }

    /// Returns a snapshot containing the clone of the current execution state.
    /// Note that the snapshot does not include slot-level information such as the slot number or block ID.
    pub(crate) fn get_snapshot(&self) -> ExecutionContextSnapshot {
        ExecutionContextSnapshot {
            ledger_changes: self.speculative_ledger.get_snapshot(),
            created_addr_index: self.created_addr_index,
            created_event_index: self.created_event_index,
            stack: self.stack.clone(),
            events: self.events.clone(),
            unsafe_rng: self.unsafe_rng.clone(),
        }
    }

    /// Resets context to an existing snapshot
    /// Note that the snapshot does not include slot-level information such as the slot number or block ID.
    pub fn reset_to_snapshot(&mut self, snapshot: ExecutionContextSnapshot) {
        self.speculative_ledger
            .reset_to_snapshot(snapshot.ledger_changes);
        self.created_addr_index = snapshot.created_addr_index;
        self.created_event_index = snapshot.created_event_index;
        self.stack = snapshot.stack;
        self.events = snapshot.events;
        self.unsafe_rng = snapshot.unsafe_rng;
    }

    /// Create a new ExecutionContext for readonly execution
    /// This should be used before performing a readonly execution.
    ///
    /// # arguments
    /// * slot: slot at which the execution will happen
    /// * req: parameters of the read only execution
    /// * previous_changes: list of ledger changes that happened since the final ledger state and before this execution
    /// * final_ledger: thread-safe access to the final ledger. Note that this will be used only for reading, never for writing
    ///
    /// # returns
    /// A ExecutionContext instance ready for a read-only execution
    pub(crate) fn readonly(
        slot: Slot,
        req: ReadOnlyExecutionRequest,
        previous_changes: LedgerChanges,
        final_ledger: Arc<RwLock<FinalLedger>>,
    ) -> Self {
        // Deterministically seed the unsafe RNG to allow the bytecode to use it.
        // Note that consecutive read-only calls for the same slot will get the same random seed.

        // Add the current slot to the seed to ensure different draws at every slot
        let mut seed: Vec<u8> = slot.to_bytes_key().to_vec();
        // Add a marker to the seed indicating that we are in read-only mode
        // to prevent random draw collisions with active executions
        seed.push(0u8); // 0u8 = read-only
        let seed = massa_hash::hash::Hash::compute_from(&seed).to_bytes();
        // We use Xoshiro256PlusPlus because it is very fast,
        // has a period long enough to ensure no repetitions will ever happen,
        // of decent quality (given the unsafe constraints)
        // but not cryptographically secure (and that's ok because the internal state is exposed anyways)
        let unsafe_rng = Xoshiro256PlusPlus::from_seed(seed);

        // return readonly context
        ExecutionContext {
            max_gas: req.max_gas,
            gas_price: req.simulated_gas_price,
            slot,
            stack: req.call_stack,
            read_only: true,
            unsafe_rng,
            ..ExecutionContext::new(final_ledger, previous_changes)
        }
    }

    /// Create a new ExecutionContext for executing an active slot.
    /// This should be used before performing any executions at that slot.
    ///
    /// # arguments
    /// * slot: slot at which the execution will happen
    /// * opt_block_id: optional ID of the block at that slot
    /// * previous_changes: list of ledger changes that happened since the final ledger state and before this execution
    /// * final_ledger: thread-safe access to the final ledger. Note that this will be used only for reading, never for writing
    ///
    /// # returns
    /// A ExecutionContext instance ready for a read-only execution
    pub(crate) fn active_slot(
        slot: Slot,
        opt_block_id: Option<BlockId>,
        previous_changes: LedgerChanges,
        final_ledger: Arc<RwLock<FinalLedger>>,
    ) -> Self {
        // Deterministically seed the unsafe RNG to allow the bytecode to use it.

        // Add the current slot to the seed to ensure different draws at every slot
        let mut seed: Vec<u8> = slot.to_bytes_key().to_vec();
        // Add a marker to the seed indicating that we are in active mode
        // to prevent random draw collisions with read-only executions
        seed.push(1u8); // 1u8 = active

        // For more deterministic entropy, seed with the block ID if any
        if let Some(block_id) = &opt_block_id {
            seed.extend(block_id.to_bytes()); // append block ID
        }
        let seed = massa_hash::hash::Hash::compute_from(&seed).to_bytes();
        let unsafe_rng = Xoshiro256PlusPlus::from_seed(seed);

        // return active slot execution context
        ExecutionContext {
            slot,
            opt_block_id,
            unsafe_rng,
            ..ExecutionContext::new(final_ledger, previous_changes)
        }
    }

    /// Moves the output of the execution out of the context,
    /// resetting some context fields in the process.
    ///
    /// This is used to get the output of an execution before discarding the context.
    /// Note that we are not taking self by value to consume it because the context is shared.
    pub fn take_execution_output(&mut self) -> ExecutionOutput {
        ExecutionOutput {
            slot: self.slot,
            block_id: std::mem::take(&mut self.opt_block_id),
            ledger_changes: self.speculative_ledger.take(),
            events: std::mem::take(&mut self.events),
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
    pub fn create_new_sc_address(&mut self, bytecode: Vec<u8>) -> Result<Address, ExecutionError> {
        // TODO: collision problem:
        //  prefix addresses to know if they are SCs or normal,
        //  otherwise people can already create new accounts by sending coins to the right hash
        //  they won't have ownership over it but this can still be unexpected
        //  to have initial extra coins when an address is created
        //  It may also induce that for read-only calls.
        //  https://github.com/massalabs/massa/issues/2331

        // deterministically generate a new unique smart contract address

        // create a seed from the current slot
        let mut data: Vec<u8> = self.slot.to_bytes_key().to_vec();
        // add the index of the created address within this context to the seed
        data.append(&mut self.created_addr_index.to_be_bytes().to_vec());
        // add a flag on whether we are in read-only mode or not to the seed
        // this prevents read-only contexts from shadowing existing addresses
        if self.read_only {
            data.push(0u8);
        } else {
            data.push(1u8);
        }
        // hash the seed to get a unique address
        let address = Address(massa_hash::hash::Hash::compute_from(&data));

        // add this address with its bytecode to the speculative ledger
        self.speculative_ledger
            .create_new_sc_address(address, bytecode)?;

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
                ))
            }
        };

        // increment the address creation counter at this slot
        self.created_addr_index += 1;

        Ok(address)
    }

    /// gets the bytecode of an address if it exists in the speculative ledger, or returns None
    pub fn get_bytecode(&self, address: &Address) -> Option<Vec<u8>> {
        self.speculative_ledger.get_bytecode(address)
    }

    /// gets the data from a datastore entry of an address if it exists in the speculative ledger, or returns None
    pub fn get_data_entry(&self, address: &Address, key: &Hash) -> Option<Vec<u8>> {
        self.speculative_ledger.get_data_entry(address, key)
    }

    /// checks if a datastore entry exists in the speculative ledger
    pub fn has_data_entry(&self, address: &Address, key: &Hash) -> bool {
        self.speculative_ledger.has_data_entry(address, key)
    }

    /// gets the bytecode of an address if it exists in the speculative ledger, or returns None
    pub fn get_parallel_balance(&self, address: &Address) -> Option<Amount> {
        self.speculative_ledger.get_parallel_balance(address)
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
        key: Hash,
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
        self.speculative_ledger.set_data_entry(address, key, data)
    }

    /// Transfers parallel coins from one address to another.
    /// No changes are retained in case of failure.
    /// Spending is only allowed from existing addresses we have write acess on
    ///
    /// # parameters
    /// * from_addr: optional spending address (use None for pure coin creation)
    /// * to_addr: optional crediting address (use None for pure coin destruction)
    /// * amount: amount of coins to transfer
    pub fn transfer_parallel_coins(
        &mut self,
        from_addr: Option<Address>,
        to_addr: Option<Address>,
        amount: Amount,
    ) -> Result<(), ExecutionError> {
        // check access rights
        if let Some(from_addr) = &from_addr {
            if !self.has_write_rights_on(from_addr) {
                return Err(ExecutionError::RuntimeError(format!(
                    "spending from address {} is not allowed in this context",
                    from_addr
                )));
            }
        }
        // do the transfer
        self.speculative_ledger
            .transfer_parallel_coins(from_addr, to_addr, amount)
    }
}
