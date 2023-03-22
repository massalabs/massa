// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module deals with executing final and active slots, as well as read-only requests.
//! It also keeps a history of executed slots, thus holding the speculative state of the ledger.
//!
//! Execution usually happens in the following way:
//! * an execution context is set up
//! * the VM is called for execution within this context
//! * the output of the execution is extracted from the context

use crate::active_history::{ActiveHistory, HistorySearchResult};
use crate::context::{ExecutionContext, ExecutionContextSnapshot};
use crate::interface_impl::InterfaceImpl;
use crate::module_cache::ModuleCache;
use crate::stats::ExecutionStatsCounter;
use massa_async_pool::AsyncMessage;
use massa_execution_exports::{
    EventStore, ExecutionConfig, ExecutionError, ExecutionOutput, ExecutionStackElement,
    ReadOnlyExecutionOutput, ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_final_state::FinalState;
use massa_ledger_exports::{SetOrDelete, SetUpdateOrDelete};
use massa_models::address::ExecutionAddressCycleInfo;
use massa_models::bytecode::Bytecode;
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::stats::ExecutionStats;
use massa_models::vesting_range::VestingRange;
use massa_models::{
    address::Address,
    block_id::BlockId,
    operation::{OperationId, OperationType, SecureShareOperation},
};
use massa_models::{amount::Amount, slot::Slot};
use massa_pos_exports::SelectorController;
use massa_sc_runtime::{Interface, Response, RuntimeModule};
use massa_storage::Storage;
use massa_time::MassaTime;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Used to acquire a lock on the execution context
macro_rules! context_guard {
    ($self:ident) => {
        $self.execution_context.lock()
    };
}

/// Structure holding consistent speculative and final execution states,
/// and allowing access to them.
pub(crate) struct ExecutionState {
    // execution config
    config: ExecutionConfig,
    // History of the outputs of recently executed slots. Slots should be consecutive, newest at the back.
    // Whenever an active slot is executed, it is appended at the back of active_history.
    // Whenever an executed active slot becomes final,
    // its output is popped from the front of active_history and applied to the final state.
    // It has atomic R/W access.
    active_history: Arc<RwLock<ActiveHistory>>,
    // a cursor pointing to the highest executed slot
    pub active_cursor: Slot,
    // a cursor pointing to the highest executed final slot
    pub final_cursor: Slot,
    // store containing execution events that became final
    final_events: EventStore,
    // final state with atomic R/W access
    final_state: Arc<RwLock<FinalState>>,
    // execution context (see documentation in context.rs)
    execution_context: Arc<Mutex<ExecutionContext>>,
    // execution interface allowing the VM runtime to access the Massa context
    execution_interface: Box<dyn Interface>,
    // execution statistics
    stats_counter: ExecutionStatsCounter,
    // cache of pre compiled sc modules
    module_cache: Arc<RwLock<ModuleCache>>,
    // Map of vesting addresses
    vesting_registry: Arc<PreHashMap<Address, Vec<VestingRange>>>,
}

impl ExecutionState {
    /// Create a new execution state. This should be called only once at the start of the execution worker.
    ///
    /// # Arguments
    /// * `config`: execution configuration
    /// * `final_state`: atomic access to the final state
    ///
    /// # returns
    /// A new `ExecutionState`
    pub fn new(config: ExecutionConfig, final_state: Arc<RwLock<FinalState>>) -> ExecutionState {
        // Get the slot at the output of which the final state is attached.
        // This should be among the latest final slots.
        let last_final_slot = final_state.read().slot;

        // Create default active history
        let active_history: Arc<RwLock<ActiveHistory>> = Default::default();

        // Initialize the map of vesting addresses from file
        let vesting_registry = match ExecutionState::init_vesting_registry(&config) {
            Ok(map) => Arc::new(map),
            Err(e) => {
                panic!("{}", e);
            }
        };

        // Initialize the SC module cache
        let module_cache = Arc::new(RwLock::new(ModuleCache::new(
            config.gas_costs.clone(),
            config.max_module_cache_size,
        )));

        // Create an empty placeholder execution context, with shared atomic access
        let execution_context = Arc::new(Mutex::new(ExecutionContext::new(
            config.clone(),
            final_state.clone(),
            active_history.clone(),
            module_cache.clone(),
            vesting_registry.clone(),
        )));

        // Instantiate the interface providing ABI access to the VM, share the execution context with it
        let execution_interface = Box::new(InterfaceImpl::new(
            config.clone(),
            execution_context.clone(),
        ));

        // build the execution state
        ExecutionState {
            final_state,
            execution_context,
            execution_interface,
            // empty execution output history: it is not recovered through bootstrap
            active_history,
            // empty final event store: it is not recovered through bootstrap
            final_events: Default::default(),
            // no active slots executed yet: set active_cursor to the last final block
            active_cursor: last_final_slot,
            final_cursor: last_final_slot,
            stats_counter: ExecutionStatsCounter::new(config.stats_time_window_duration),
            module_cache,
            config,
            vesting_registry,
        }
    }

    /// Get execution statistics
    pub fn get_stats(&self) -> ExecutionStats {
        self.stats_counter.get_stats(self.active_cursor)
    }

    /// Applies the output of an execution to the final execution state.
    /// The newly applied final output should be from the slot just after the last executed final slot
    ///
    /// # Arguments
    /// * `exec_out`: execution output to apply
    pub fn apply_final_execution_output(&mut self, mut exec_out: ExecutionOutput) {
        if self.final_cursor >= exec_out.slot {
            panic!("attempting to apply a final execution output at or before the current final_cursor");
        }

        // count stats
        if exec_out.block_id.is_some() {
            self.stats_counter.register_final_blocks(1);
            self.stats_counter.register_final_executed_operations(
                exec_out.state_changes.executed_ops_changes.len(),
            );
        }

        // apply state changes to the final ledger
        self.final_state
            .write()
            .finalize(exec_out.slot, exec_out.state_changes);

        // update the final ledger's slot
        self.final_cursor = exec_out.slot;

        // update active cursor:
        // if it was at the previous latest final block, set it to point to the new one
        if self.active_cursor < self.final_cursor {
            self.active_cursor = self.final_cursor;
        }

        // append generated events to the final event store
        exec_out.events.finalize();
        self.final_events.extend(exec_out.events);
        self.final_events.prune(self.config.max_final_events);
    }

    /// Applies an execution output to the active (non-final) state
    /// The newly active final output should be from the slot just after the last executed active slot
    ///
    /// # Arguments
    /// * `exec_out`: execution output to apply
    pub fn apply_active_execution_output(&mut self, exec_out: ExecutionOutput) {
        if self.active_cursor >= exec_out.slot {
            panic!("attempting to apply an active execution output at or before the current active_cursor");
        }
        if exec_out.slot <= self.final_cursor {
            panic!("attempting to apply an active execution output at or before the current final_cursor");
        }

        // update active cursor to reflect the new latest active slot
        self.active_cursor = exec_out.slot;

        // add the execution output at the end of the output history
        self.active_history.write().0.push_back(exec_out);
    }

    /// Helper function.
    /// Within a locked execution context (lock is taken at the beginning of the function then released at the end):
    /// - if not yet executed then transfer fee and add the operation to the context then return a context snapshot
    ///
    /// # Arguments
    /// * `operation`: operation to be schedule
    /// * `sender_addr`: sender address for the operation (for fee transfer)

    fn schedule_operation_for_execution(
        &self,
        operation: &SecureShareOperation,
        sender_addr: Address,
    ) -> Result<ExecutionContextSnapshot, ExecutionError> {
        let operation_id = operation.id;

        // lock execution context
        let mut context = context_guard!(self);

        // ignore the operation if it was already executed
        if context.is_op_executed(&operation_id) {
            return Err(ExecutionError::IncludeOperationError(
                "operation was executed previously".to_string(),
            ));
        }

        // debit the fee from the operation sender
        // fail execution if there are not enough coins
        if let Err(err) =
            context.transfer_coins(Some(sender_addr), None, operation.content.fee, false)
        {
            return Err(ExecutionError::IncludeOperationError(format!(
                "could not spend fees: {}",
                err
            )));
        }

        // from here, fees transferred. Op will be executed just after in the context of a snapshot.

        // save a snapshot of the context to revert any further changes on error
        let context_snapshot = context.get_snapshot();

        // set the context max gas to match the one defined in the operation
        context.max_gas = operation.get_gas_usage();

        // set the creator address
        context.creator_address = Some(operation.content_creator_address);

        // set the context origin operation ID
        context.origin_operation_id = Some(operation_id);

        Ok(context_snapshot)
    }

    /// Execute an operation in the context of a block.
    /// Assumes the execution context was initialized at the beginning of the slot.
    ///
    /// # Arguments
    /// * `operation`: operation to execute
    /// * `block_slot`: slot of the block in which the op is included
    /// * `remaining_block_gas`: mutable reference towards the remaining gas in the block
    /// * `block_credits`: mutable reference towards the total block reward/fee credits
    pub fn execute_operation(
        &self,
        operation: &SecureShareOperation,
        block_slot: Slot,
        remaining_block_gas: &mut u64,
        block_credits: &mut Amount,
    ) -> Result<(), ExecutionError> {
        // check validity period
        if !(operation
            .get_validity_range(self.config.operation_validity_period)
            .contains(&block_slot.period))
        {
            return Err(ExecutionError::InvalidSlotRange);
        }

        // check remaining block gas
        let op_gas = operation.get_gas_usage();
        let new_remaining_block_gas = remaining_block_gas.checked_sub(op_gas).ok_or_else(|| {
            ExecutionError::NotEnoughGas(
                "not enough remaining block gas to execute operation".to_string(),
            )
        })?;

        // get the operation's sender address
        let sender_addr = operation.content_creator_address;

        // get the thread to which the operation belongs
        let op_thread = sender_addr.get_thread(self.config.thread_count);

        // check block/op thread compatibility
        if op_thread != block_slot.thread {
            return Err(ExecutionError::IncludeOperationError(
                "operation vs block thread mismatch".to_string(),
            ));
        }

        // get operation ID
        let operation_id = operation.id;

        // Add fee from operation.
        let new_block_credits = block_credits.saturating_add(operation.content.fee);

        let context_snapshot = self.schedule_operation_for_execution(operation, sender_addr)?;

        // update block gas
        *remaining_block_gas = new_remaining_block_gas;

        // update block credits
        *block_credits = new_block_credits;

        // Call the execution process specific to the operation type.
        let execution_result = match &operation.content.op {
            OperationType::ExecuteSC { .. } => {
                self.execute_executesc_op(&operation.content.op, sender_addr)
            }
            OperationType::CallSC { .. } => {
                self.execute_callsc_op(&operation.content.op, sender_addr)
            }
            OperationType::RollBuy { .. } => {
                self.execute_roll_buy_op(&operation.content.op, sender_addr, block_slot)
            }
            OperationType::RollSell { .. } => {
                self.execute_roll_sell_op(&operation.content.op, sender_addr)
            }
            OperationType::Transaction { .. } => {
                self.execute_transaction_op(&operation.content.op, sender_addr)
            }
        };

        {
            // lock execution context
            let mut context = context_guard!(self);

            // check execution results
            match execution_result {
                Ok(_) => context.insert_executed_op(
                    operation_id,
                    true,
                    Slot::new(operation.content.expire_period, op_thread),
                ),
                Err(err) => {
                    // an error occurred: emit error event and reset context to snapshot
                    let err = ExecutionError::RuntimeError(format!(
                        "runtime error when executing operation {}: {}",
                        operation_id, &err
                    ));
                    debug!("{}", &err);
                    context.reset_to_snapshot(context_snapshot, err);

                    // Insert op AFTER the context has been restore (else it would be overwritten)
                    context.insert_executed_op(
                        operation_id,
                        false,
                        Slot::new(operation.content.expire_period, op_thread),
                    )
                }
            }
        }

        Ok(())
    }

    /// Execute an operation of type `RollSell`
    /// Will panic if called with another operation type
    ///
    /// # Arguments
    /// * `operation`: the `WrappedOperation` to process, must be an `RollSell`
    /// * `sender_addr`: address of the sender
    pub fn execute_roll_sell_op(
        &self,
        operation: &OperationType,
        seller_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process roll sell operations only
        let roll_count = match operation {
            OperationType::RollSell { roll_count } => roll_count,
            _ => panic!("unexpected operation type"),
        };

        // acquire write access to the context
        let mut context = context_guard!(self);

        // Set call stack
        // This needs to be defined before anything can fail, so that the emitted event contains the right stack
        context.stack = vec![ExecutionStackElement {
            address: seller_addr,
            coins: Amount::default(),
            owned_addresses: vec![seller_addr],
            operation_datastore: None,
        }];

        // try to sell the rolls
        if let Err(err) = context.try_sell_rolls(&seller_addr, *roll_count) {
            return Err(ExecutionError::RollSellError(format!(
                "{} failed to sell {} rolls: {}",
                seller_addr, roll_count, err
            )));
        }
        Ok(())
    }

    /// Execute an operation of type `RollBuy`
    /// Will panic if called with another operation type
    ///
    /// # Arguments
    /// * `operation`: the `WrappedOperation` to process, must be an `RollBuy`
    /// * `sender_addr`: address of the sender
    pub fn execute_roll_buy_op(
        &self,
        operation: &OperationType,
        buyer_addr: Address,
        current_slot: Slot,
    ) -> Result<(), ExecutionError> {
        // process roll buy operations only
        let roll_count = match operation {
            OperationType::RollBuy { roll_count } => roll_count,
            _ => panic!("unexpected operation type"),
        };

        // control vesting max_rolls for buyer address
        if let Some(vesting_range) = self.find_vesting_range(&buyer_addr, &current_slot) {
            let rolls = self.get_final_and_candidate_rolls(&buyer_addr);
            // (candidate_rolls + amount to buy)
            let max_rolls = rolls.1.saturating_add(*roll_count);
            if max_rolls > vesting_range.max_rolls {
                return Err(ExecutionError::VestingError(format!(
                    "trying to get to a total of {} rolls but only {} are allowed at that time by the vesting scheme",
                    max_rolls, vesting_range.max_rolls
                )));
            }
        }

        // acquire write access to the context
        let mut context = context_guard!(self);

        // Set call stack
        // This needs to be defined before anything can fail, so that the emitted event contains the right stack
        context.stack = vec![ExecutionStackElement {
            address: buyer_addr,
            coins: Default::default(),
            owned_addresses: vec![buyer_addr],
            operation_datastore: None,
        }];

        // compute the amount of coins to spend
        let spend_coins = match self.config.roll_price.checked_mul_u64(*roll_count) {
            Some(v) => v,
            None => {
                return Err(ExecutionError::RollBuyError(format!(
                    "{} failed to buy {} rolls: overflow on the required coin amount",
                    buyer_addr, roll_count
                )));
            }
        };

        // spend `roll_price` * `roll_count` coins from the buyer
        if let Err(err) = context.transfer_coins(Some(buyer_addr), None, spend_coins, false) {
            return Err(ExecutionError::RollBuyError(format!(
                "{} failed to buy {} rolls: {}",
                buyer_addr, roll_count, err
            )));
        }

        // add rolls to the buyer withing the context
        context.add_rolls(&buyer_addr, *roll_count);

        Ok(())
    }

    /// Execute an operation of type `Transaction`
    /// Will panic if called with another operation type
    ///
    /// # Arguments
    /// * `operation`: the `WrappedOperation` to process, must be a `Transaction`
    /// * `operation_id`: ID of the operation
    /// * `sender_addr`: address of the sender
    pub fn execute_transaction_op(
        &self,
        operation: &OperationType,
        sender_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process transaction operations only
        let (recipient_address, amount) = match operation {
            OperationType::Transaction {
                recipient_address,
                amount,
            } => (recipient_address, amount),
            _ => panic!("unexpected operation type"),
        };

        // acquire write access to the context
        let mut context = context_guard!(self);

        // Set call stack
        // This needs to be defined before anything can fail, so that the emitted event contains the right stack
        context.stack = vec![ExecutionStackElement {
            address: sender_addr,
            coins: *amount,
            owned_addresses: vec![sender_addr],
            operation_datastore: None,
        }];

        // send `roll_price` * `roll_count` coins from the sender to the recipient
        if let Err(err) =
            context.transfer_coins(Some(sender_addr), Some(*recipient_address), *amount, false)
        {
            return Err(ExecutionError::TransactionError(format!(
                "transfer of {} coins from {} to {} failed: {}",
                amount, sender_addr, recipient_address, err
            )));
        }

        Ok(())
    }

    /// Execute an operation of type `ExecuteSC`
    /// Will panic if called with another operation type
    ///
    /// # Arguments
    /// * `operation`: the `WrappedOperation` to process, must be an `ExecuteSC`
    /// * `sender_addr`: address of the sender
    pub fn execute_executesc_op(
        &self,
        operation: &OperationType,
        sender_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process ExecuteSC operations only
        let (bytecode, max_gas, datastore) = match &operation {
            OperationType::ExecuteSC {
                data,
                max_gas,
                datastore,
                ..
            } => (data, max_gas, datastore),
            _ => panic!("unexpected operation type"),
        };

        {
            // acquire write access to the context
            let mut context = context_guard!(self);

            // Set the call stack to a single element:
            // * the execution will happen in the context of the address of the operation's sender
            // * the context will give the operation's sender write access to its own ledger entry
            // This needs to be defined before anything can fail, so that the emitted event contains the right stack
            context.stack = vec![ExecutionStackElement {
                address: sender_addr,
                coins: Amount::zero(),
                owned_addresses: vec![sender_addr],
                operation_datastore: Some(datastore.clone()),
            }];
        };

        // run the VM on the bytecode contained in the operation
        let module = RuntimeModule::new(bytecode, *max_gas, self.config.gas_costs.clone())
            .map_err(|err| {
                ExecutionError::RuntimeError(format!(
                    "compilation error in execute_executesc_op: {}",
                    err
                ))
            })?;
        match massa_sc_runtime::run_main(
            &*self.execution_interface,
            module,
            *max_gas,
            self.config.gas_costs.clone(),
        ) {
            Ok(_response) => {}
            Err(err) => {
                return Err(ExecutionError::RuntimeError(format!(
                    "module execution error in execute_executesc_op: {}",
                    err
                )));
            }
        }

        Ok(())
    }

    /// Execute an operation of type `CallSC`
    /// Will panic if called with another operation type
    ///
    /// # Arguments
    /// * `operation`: the `WrappedOperation` to process, must be an `CallSC`
    /// * `block_creator_addr`: address of the block creator
    /// * `operation_id`: ID of the operation
    /// * `sender_addr`: address of the sender
    pub fn execute_callsc_op(
        &self,
        operation: &OperationType,
        sender_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process CallSC operations only
        let (max_gas, target_addr, target_func, param, coins) = match &operation {
            OperationType::CallSC {
                max_gas,
                target_addr,
                target_func,
                param,
                coins,
                ..
            } => (*max_gas, *target_addr, target_func, param, *coins),
            _ => panic!("unexpected operation type"),
        };

        // prepare the current slot context for executing the operation
        let bytecode;
        {
            // acquire write access to the context
            let mut context = context_guard!(self);

            // Set the call stack
            // This needs to be defined before anything can fail, so that the emitted event contains the right stack
            context.stack = vec![
                ExecutionStackElement {
                    address: sender_addr,
                    coins: Default::default(),
                    owned_addresses: vec![sender_addr],
                    operation_datastore: None,
                },
                ExecutionStackElement {
                    address: target_addr,
                    coins,
                    owned_addresses: vec![target_addr],
                    operation_datastore: None,
                },
            ];

            // Debit the sender's balance with the coins to transfer
            if let Err(err) = context.transfer_coins(Some(sender_addr), None, coins, false) {
                return Err(ExecutionError::RuntimeError(format!(
                    "failed to debit operation sender {} with {} operation coins: {}",
                    sender_addr, coins, err
                )));
            }

            // Credit the operation target with coins.
            if let Err(err) = context.transfer_coins(None, Some(target_addr), coins, false) {
                return Err(ExecutionError::RuntimeError(format!(
                    "failed to credit operation target {} with {} operation coins: {}",
                    target_addr, coins, err
                )));
            }

            // quit if there is no function to be called
            if target_func.is_empty() {
                return Ok(());
            }

            // Load bytecode. Assume empty bytecode if not found.
            bytecode = context.get_bytecode(&target_addr).unwrap_or_default().0;
        }

        // Execute bytecode
        // IMPORTANT: do not keep a lock here as `run_function` uses the `get_module` interface
        let module = self.module_cache.write().get_module(&bytecode, max_gas)?;
        match massa_sc_runtime::run_function(
            &*self.execution_interface,
            module.clone(),
            target_func,
            param,
            max_gas,
            self.config.gas_costs.clone(),
        ) {
            Ok(Response { init_cost, .. }) => {
                self.module_cache
                    .write()
                    .save_module(&bytecode, module, init_cost);
                Ok(())
            }
            Err(err) => Err(ExecutionError::RuntimeError(format!(
                "module execution error in execute_callsc_op: {}",
                err
            ))),
        }
    }

    /// Tries to execute an asynchronous message
    /// If the execution failed reimburse the message sender.
    ///
    /// # Arguments
    /// * message: message information
    /// * bytecode: executable target bytecode, or None if unavailable
    pub fn execute_async_message(
        &self,
        message: AsyncMessage,
        bytecode: Option<Bytecode>,
    ) -> Result<(), ExecutionError> {
        // prepare execution context
        let context_snapshot;
        let bytecode: Bytecode = {
            let mut context = context_guard!(self);
            context_snapshot = context.get_snapshot();
            context.max_gas = message.max_gas;
            context.creator_address = None;
            context.stack = vec![
                ExecutionStackElement {
                    address: message.sender,
                    coins: message.coins,
                    owned_addresses: vec![message.sender],
                    operation_datastore: None,
                },
                ExecutionStackElement {
                    address: message.destination,
                    coins: message.coins,
                    owned_addresses: vec![message.destination],
                    operation_datastore: None,
                },
            ];

            // If there is no target bytecode or if message data is invalid,
            // reimburse sender with coins and quit
            let bytecode = match bytecode {
                Some(bc) => bc,
                bc => {
                    let err = if bc.is_none() {
                        ExecutionError::RuntimeError("no target bytecode found".into())
                    } else {
                        ExecutionError::RuntimeError(
                            "message data does not convert to utf-8".into(),
                        )
                    };
                    context.reset_to_snapshot(context_snapshot, err.clone());
                    context.cancel_async_message(&message);
                    return Err(err);
                }
            };

            // credit coins to the target address
            if let Err(err) =
                context.transfer_coins(None, Some(message.destination), message.coins, false)
            {
                // coin crediting failed: reset context to snapshot and reimburse sender
                let err = ExecutionError::RuntimeError(format!(
                    "could not credit coins to target of async execution: {}",
                    err
                ));
                context.reset_to_snapshot(context_snapshot, err.clone());
                context.cancel_async_message(&message);
                return Err(err);
            }

            bytecode
        };

        // Execute bytecode
        // IMPORTANT: do not keep a lock here as `run_function` uses the `get_module` interface
        let module = self
            .module_cache
            .write()
            .get_module(&bytecode.0, message.max_gas)?;
        match massa_sc_runtime::run_function(
            &*self.execution_interface,
            module.clone(),
            &message.handler,
            &message.data,
            message.max_gas,
            self.config.gas_costs.clone(),
        ) {
            Ok(Response { init_cost, .. }) => {
                self.module_cache
                    .write()
                    .save_module(&bytecode.0, module, init_cost);
                Ok(())
            }
            Err(err) => {
                // execution failed: reset context to snapshot and reimburse sender
                let err = ExecutionError::RuntimeError(format!(
                    "module execution error in execute_async_message: {}",
                    err
                ));
                let mut context = context_guard!(self);
                context.reset_to_snapshot(context_snapshot, err.clone());
                context.cancel_async_message(&message);
                Err(err)
            }
        }
    }

    /// Executes a full slot (with or without a block inside) without causing any changes to the state,
    /// just yielding the execution output.
    ///
    /// # Arguments
    /// * `slot`: slot to execute
    /// * `opt_block`: Storage owning a ref to the block (+ its endorsements, ops, parents) if there is a block a that slot, otherwise None
    /// * `selector`: Reference to the selector
    ///
    /// # Returns
    /// An `ExecutionOutput` structure summarizing the output of the executed slot
    pub fn execute_slot(
        &self,
        slot: &Slot,
        exec_target: Option<&(BlockId, Storage)>,
        selector: Box<dyn SelectorController>,
    ) -> ExecutionOutput {
        // Create a new execution context for the whole active slot
        let mut execution_context = ExecutionContext::active_slot(
            self.config.clone(),
            *slot,
            exec_target.as_ref().map(|(b_id, _)| *b_id),
            self.final_state.clone(),
            self.active_history.clone(),
            self.module_cache.clone(),
            self.vesting_registry.clone(),
        );

        // Get asynchronous messages to execute
        let messages = execution_context.take_async_batch(self.config.max_async_gas);
        debug!("executing {} messages at slot {}", messages.len(), slot);

        // Apply the created execution context for slot execution
        *context_guard!(self) = execution_context;

        // Try executing asynchronous messages.
        // Effects are cancelled on failure and the sender is reimbursed.
        for (opt_bytecode, message) in messages {
            if let Err(err) = self.execute_async_message(message, opt_bytecode) {
                debug!("failed executing async message: {}", err);
            }
        }

        // Check if there is a block at this slot
        if let Some((block_id, block_store)) = exec_target {
            // Retrieve the block from storage
            let stored_block = block_store
                .read_blocks()
                .get(block_id)
                .expect("Missing block in storage.")
                .clone();

            // gather all operations
            let operations = {
                let ops = block_store.read_operations();
                stored_block
                    .content
                    .operations
                    .into_iter()
                    .map(|op_id| {
                        ops.get(&op_id)
                            .expect("block operation absent from storage")
                            .clone()
                    })
                    .collect::<Vec<_>>()
            };

            debug!("executing {} operations at slot {}", operations.len(), slot);

            // gather all available endorsement creators and target blocks
            let (endorsement_creators, endorsement_targets): &(Vec<Address>, Vec<BlockId>) =
                &stored_block
                    .content
                    .header
                    .content
                    .endorsements
                    .iter()
                    .map(|endo| (endo.content_creator_address, endo.content.endorsed_block))
                    .unzip();

            // deduce endorsement target block creators
            let endorsement_target_creators = {
                let blocks = block_store.read_blocks();
                endorsement_targets
                    .iter()
                    .map(|b_id| {
                        blocks
                            .get(b_id)
                            .expect("endorsed block absent from storage")
                            .content_creator_address
                    })
                    .collect::<Vec<_>>()
            };

            // Set remaining block gas
            let mut remaining_block_gas = self.config.max_gas_per_block;

            // Set block credits
            let mut block_credits = self.config.block_reward;

            // Try executing the operations of this block in the order in which they appear in the block.
            // Errors are logged but do not interrupt the execution of the slot.
            for operation in operations.into_iter() {
                if let Err(err) = self.execute_operation(
                    &operation,
                    stored_block.content.header.content.slot,
                    &mut remaining_block_gas,
                    &mut block_credits,
                ) {
                    debug!(
                        "failed executing operation {} in block {}: {}",
                        operation.id, block_id, err
                    );
                }
            }

            // Get block creator address
            let block_creator_addr = stored_block.content_creator_address;

            // acquire lock on execution context
            let mut context = context_guard!(self);

            // Update speculative rolls state production stats
            context.update_production_stats(&block_creator_addr, *slot, Some(*block_id));

            // Credit endorsement producers and endorsed block producers
            let mut remaining_credit = block_credits;
            let block_credit_part = block_credits
                .checked_div_u64(3 * (1 + (self.config.endorsement_count)))
                .expect("critical: block_credits checked_div factor is 0");
            for (endorsement_creator, endorsement_target_creator) in endorsement_creators
                .iter()
                .zip(endorsement_target_creators.into_iter())
            {
                // credit creator of the endorsement with coins
                match context.transfer_coins(
                    None,
                    Some(*endorsement_creator),
                    block_credit_part,
                    false,
                ) {
                    Ok(_) => {
                        remaining_credit = remaining_credit.saturating_sub(block_credit_part);
                    }
                    Err(err) => {
                        debug!(
                            "failed to credit {} coins to endorsement creator {} for an endorsed block execution: {}",
                            block_credit_part, endorsement_creator, err
                        )
                    }
                }

                // credit creator of the endorsed block with coins
                match context.transfer_coins(
                    None,
                    Some(endorsement_target_creator),
                    block_credit_part,
                    false,
                ) {
                    Ok(_) => {
                        remaining_credit = remaining_credit.saturating_sub(block_credit_part);
                    }
                    Err(err) => {
                        debug!(
                            "failed to credit {} coins to endorsement target creator {} on block execution: {}",
                            block_credit_part, endorsement_target_creator, err
                        )
                    }
                }
            }

            // Credit block creator with remaining_credit
            if let Err(err) =
                context.transfer_coins(None, Some(block_creator_addr), remaining_credit, false)
            {
                debug!(
                    "failed to credit {} coins to block creator {} on block execution: {}",
                    remaining_credit, block_creator_addr, err
                )
            }
        } else {
            // the slot is a miss, check who was supposed to be the creator and update production stats
            let producer_addr = selector
                .get_producer(*slot)
                .expect("couldn't get the expected block producer for a missed slot");
            context_guard!(self).update_production_stats(&producer_addr, *slot, None);
        }

        // Finish slot and return the execution output
        context_guard!(self).settle_slot()
    }

    /// Execute a candidate slot
    pub fn execute_candidate_slot(
        &mut self,
        slot: &Slot,
        exec_target: Option<&(BlockId, Storage)>,
        selector: Box<dyn SelectorController>,
    ) {
        let target_id = exec_target.as_ref().map(|(b_id, _)| *b_id);
        debug!(
            "execute_candidate_slot: executing slot={} target={:?}",
            slot, target_id
        );

        if slot <= &self.final_cursor {
            panic!(
                "could not execute candidate slot {} because final_cursor is at {}",
                slot, self.final_cursor
            );
        }

        // if the slot was already executed, truncate active history to cancel the slot and all the ones after
        if &self.active_cursor >= slot {
            debug!(
                "execute_candidate_slot: truncating down from slot {}",
                self.active_cursor
            );
            self.active_history
                .write()
                .truncate_from(slot, self.config.thread_count);
            self.active_cursor = slot
                .get_prev_slot(self.config.thread_count)
                .expect("overflow when iterating on slots");
        }

        let exec_out = self.execute_slot(slot, exec_target, selector);

        // apply execution output to active state
        self.apply_active_execution_output(exec_out);
        debug!("execute_candidate_slot: execution finished & state applied");
    }

    /// Execute an SCE-final slot
    pub fn execute_final_slot(
        &mut self,
        slot: &Slot,
        exec_target: Option<&(BlockId, Storage)>,
        selector: Box<dyn SelectorController>,
    ) {
        let target_id = exec_target.as_ref().map(|(b_id, _)| *b_id);
        debug!(
            "execute_final_slot: executing slot={} target={:?}",
            slot, target_id
        );

        if slot <= &self.final_cursor {
            debug!(
                "execute_final_slot: final slot already executed (final_cursor = {})",
                self.final_cursor
            );
            return;
        }

        // check if the final slot execution result is already cached at the front of the speculative execution history
        let first_exec_output = self.active_history.write().0.pop_front();
        if let Some(exec_out) = first_exec_output {
            if &exec_out.slot == slot && exec_out.block_id == target_id {
                // speculative execution front result matches what we want to compute

                // apply the cached output and return
                self.apply_final_execution_output(exec_out);

                debug!("execute_final_slot: found in cache, applied cache");
                return;
            } else {
                // speculative cache mismatch
                warn!(
                    "speculative execution cache mismatch (final slot={}/block={:?}, front speculative slot={}/block={:?}). Resetting the cache.",
                    slot, target_id, exec_out.slot, exec_out.block_id
                );
            }
        } else {
            // cache entry absent
            info!(
                "speculative execution cache empty, executing final slot={}/block={:?}",
                slot, target_id
            );
        }

        // truncate the whole execution queue
        self.active_history.write().0.clear();
        self.active_cursor = self.final_cursor;

        // execute slot
        debug!("execute_final_slot: execution started");
        let exec_out = self.execute_slot(slot, exec_target, selector);

        // apply execution output to final state
        self.apply_final_execution_output(exec_out);
        debug!("execute_final_slot: execution finished & result applied");
    }

    /// Runs a read-only execution request.
    /// The executed bytecode appears to be able to read and write the consensus state,
    /// but all accumulated changes are simply returned as an `ExecutionOutput` object,
    /// and not actually applied to the consensus state.
    ///
    /// # Arguments
    /// * `req`: a read-only execution request
    ///
    /// # Returns
    ///  `ExecutionOutput` describing the output of the execution, or an error
    pub(crate) fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ReadOnlyExecutionOutput, ExecutionError> {
        // TODO ensure that speculative things are reset after every execution ends (incl. on error and readonly)
        // otherwise, on prod stats accumulation etc... from the API we might be counting the remainder of this speculative execution

        // check if read only request max gas is above the threshold
        if req.max_gas > self.config.max_read_only_gas {
            return Err(ExecutionError::TooMuchGas(format!(
                "execution gas for read-only call is {} which is above the maximum allowed {}",
                req.max_gas, self.config.max_read_only_gas
            )));
        }

        // set the execution slot to be the one after the latest executed active or final slot
        let slot = if req.is_final {
            self.final_cursor
                .get_next_slot(self.config.thread_count)
                .expect("slot overflow in readonly execution from final slot")
        } else {
            self.active_cursor
                .get_next_slot(self.config.thread_count)
                .expect("slot overflow in readonly execution from active slot")
        };

        // create a readonly execution context
        let execution_context = ExecutionContext::readonly(
            self.config.clone(),
            slot,
            req.max_gas,
            req.call_stack,
            self.final_state.clone(),
            self.active_history.clone(),
            self.module_cache.clone(),
            self.vesting_registry.clone(),
        );

        // run the interpreter according to the target type
        let exec_response = match req.target {
            ReadOnlyExecutionTarget::BytecodeExecution(bytecode) => {
                // set the execution context for execution
                *context_guard!(self) = execution_context;

                // run the bytecode's main function
                let module =
                    RuntimeModule::new(&bytecode, req.max_gas, self.config.gas_costs.clone())
                        .map_err(|err| {
                            ExecutionError::RuntimeError(format!(
                                "compilation error in execute_readonly_request: {}",
                                err
                            ))
                        })?;
                massa_sc_runtime::run_main(
                    &*self.execution_interface,
                    module,
                    req.max_gas,
                    self.config.gas_costs.clone(),
                )
                .map_err(|err| {
                    ExecutionError::RuntimeError(format!(
                        "module execution error in execute_readonly_request BytecodeExecution: {}",
                        err,
                    ))
                })?
            }
            ReadOnlyExecutionTarget::FunctionCall {
                target_addr,
                target_func,
                parameter,
            } => {
                // get the bytecode, default to an empty vector
                let bytecode = execution_context
                    .get_bytecode(&target_addr)
                    .unwrap_or_default()
                    .0;

                // set the execution context for execution
                *context_guard!(self) = execution_context;

                // Execute bytecode
                // IMPORTANT: do not keep a lock here as `run_function` uses the `get_module` interface
                let module = self
                    .module_cache
                    .write()
                    .get_module(&bytecode, req.max_gas)?;
                let response = massa_sc_runtime::run_function(
                    &*self.execution_interface,
                    module.clone(),
                    &target_func,
                    &parameter,
                    req.max_gas,
                    self.config.gas_costs.clone(),
                )
                .map_err(|err| {
                    ExecutionError::RuntimeError(format!(
                        "module execution error in execute_readonly_request BytecodeExecution: {}",
                        err,
                    ))
                })?;
                self.module_cache
                    .write()
                    .save_module(&bytecode, module, response.init_cost);
                response
            }
        };

        // return the execution output
        let execution_output = context_guard!(self).settle_slot();
        Ok(ReadOnlyExecutionOutput {
            out: execution_output,
            gas_cost: req.max_gas.saturating_sub(exec_response.remaining_gas),
            call_result: exec_response.ret,
        })
    }

    /// Gets a balance both at the latest final and candidate executed slots
    pub fn get_final_and_candidate_balance(
        &self,
        address: &Address,
    ) -> (Option<Amount>, Option<Amount>) {
        let final_balance = self.final_state.read().ledger.get_balance(address);
        let search_result = self.active_history.read().fetch_balance(address);
        (
            final_balance,
            match search_result {
                HistorySearchResult::Present(active_balance) => Some(active_balance),
                HistorySearchResult::NoInfo => final_balance,
                HistorySearchResult::Absent => None,
            },
        )
    }

    /// Gets roll counts both at the latest final and active executed slots
    pub fn get_final_and_candidate_rolls(&self, address: &Address) -> (u64, u64) {
        let final_rolls = self.final_state.read().pos_state.get_rolls_for(address);
        let active_rolls = self
            .active_history
            .read()
            .fetch_roll_count(address)
            .unwrap_or(final_rolls);
        (final_rolls, active_rolls)
    }

    /// Gets a data entry both at the latest final and active executed slots
    pub fn get_final_and_active_data_entry(
        &self,
        address: &Address,
        key: &[u8],
    ) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        let final_entry = self.final_state.read().ledger.get_data_entry(address, key);
        let search_result = self
            .active_history
            .read()
            .fetch_active_history_data_entry(address, key);
        (
            final_entry.clone(),
            match search_result {
                HistorySearchResult::Present(active_entry) => Some(active_entry),
                HistorySearchResult::NoInfo => final_entry,
                HistorySearchResult::Absent => None,
            },
        )
    }

    /// Get every final and active datastore key of the given address
    pub fn get_final_and_candidate_datastore_keys(
        &self,
        addr: &Address,
    ) -> (BTreeSet<Vec<u8>>, BTreeSet<Vec<u8>>) {
        // here, get the final keys from the final ledger, and make a copy of it for the candidate list
        // let final_keys = final_state.read().ledger.get_datastore_keys(addr);
        let final_keys = self
            .final_state
            .read()
            .ledger
            .get_datastore_keys(addr)
            .unwrap_or_default();
        let mut candidate_keys = final_keys.clone();

        // here, traverse the history from oldest to newest, applying additions and deletions
        for output in &self.active_history.read().0 {
            match output.state_changes.ledger_changes.get(addr) {
                // address absent from the changes
                None => (),

                // address ledger entry being reset to an absolute new list of keys
                Some(SetUpdateOrDelete::Set(new_ledger_entry)) => {
                    candidate_keys = new_ledger_entry.datastore.keys().cloned().collect();
                }

                // address ledger entry being updated
                Some(SetUpdateOrDelete::Update(entry_updates)) => {
                    for (ds_key, ds_update) in &entry_updates.datastore {
                        match ds_update {
                            SetOrDelete::Set(_) => candidate_keys.insert(ds_key.clone()),
                            SetOrDelete::Delete => candidate_keys.remove(ds_key),
                        };
                    }
                }

                // address ledger entry being deleted
                Some(SetUpdateOrDelete::Delete) => {
                    candidate_keys.clear();
                }
            }
        }

        (final_keys, candidate_keys)
    }

    /// Returns for a given cycle the stakers taken into account
    /// by the selector. That correspond to the `roll_counts` in `cycle - 3`.
    ///
    /// By default it returns an empty map.
    pub fn get_cycle_active_rolls(&self, cycle: u64) -> BTreeMap<Address, u64> {
        let lookback_cycle = cycle.checked_sub(3);
        let final_state = self.final_state.read();
        if let Some(lookback_cycle) = lookback_cycle {
            let lookback_cycle_index = match final_state.pos_state.get_cycle_index(lookback_cycle) {
                Some(v) => v,
                None => Default::default(),
            };
            final_state.pos_state.cycle_history[lookback_cycle_index]
                .roll_counts
                .clone()
        } else {
            final_state.pos_state.initial_rolls.clone()
        }
    }

    /// Gets execution events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    /// * event state (final, candidate or both)
    pub fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent> {
        match filter.is_final {
            Some(true) => self
                .final_events
                .get_filtered_sc_output_events(&filter)
                .into_iter()
                .collect(),
            Some(false) => self
                .active_history
                .read()
                .0
                .iter()
                .flat_map(|item| item.events.get_filtered_sc_output_events(&filter))
                .collect(),
            None => self
                .final_events
                .get_filtered_sc_output_events(&filter)
                .into_iter()
                .chain(
                    self.active_history
                        .read()
                        .0
                        .iter()
                        .flat_map(|item| item.events.get_filtered_sc_output_events(&filter)),
                )
                .collect(),
        }
    }

    /// List which operations inside the provided list were not executed
    pub fn unexecuted_ops_among(
        &self,
        ops: &PreHashSet<OperationId>,
        thread: u8,
    ) -> PreHashSet<OperationId> {
        let mut ops = ops.clone();

        if ops.is_empty() {
            return ops;
        }

        {
            // check active history
            let history = self.active_history.read();
            for hist_item in history.0.iter().rev() {
                if hist_item.slot.thread != thread {
                    continue;
                }
                ops.retain(|op_id| {
                    !hist_item
                        .state_changes
                        .executed_ops_changes
                        .contains_key(op_id)
                });
                if ops.is_empty() {
                    return ops;
                }
            }
        }

        {
            // check final state
            let final_state = self.final_state.read();
            ops.retain(|op_id| !final_state.executed_ops.contains(op_id));
        }

        ops
    }

    /// Gets the production stats for an address at all cycles
    pub fn get_address_cycle_infos(&self, address: &Address) -> Vec<ExecutionAddressCycleInfo> {
        context_guard!(self).get_address_cycle_infos(address, self.config.periods_per_cycle)
    }

    /// Get future deferred credits of an address
    pub fn get_address_future_deferred_credits(&self, address: &Address) -> BTreeMap<Slot, Amount> {
        context_guard!(self).get_address_future_deferred_credits(address, self.config.thread_count)
    }

    /// Get the execution statuses of both speculative and final executions
    ///
    /// # Return
    ///
    /// * A tuple of hashmaps with:
    /// * first the statuses for speculative executions
    /// * second the statuses for final executions
    pub fn get_op_exec_status(&self) -> (HashMap<OperationId, bool>, HashMap<OperationId, bool>) {
        (
            self.active_history.read().get_op_exec_status(),
            self.final_state.read().executed_ops.op_exec_status.clone(),
        )
    }

    /// Initialize the hashmap of addresses from the vesting file
    pub fn init_vesting_registry(
        config: &ExecutionConfig,
    ) -> Result<PreHashMap<Address, Vec<VestingRange>>, ExecutionError> {
        let mut hashmap: PreHashMap<Address, Vec<VestingRange>> = serde_json::from_str(
            &std::fs::read_to_string(&config.initial_vesting_path).map_err(|err| {
                ExecutionError::InitVestingError(format!(
                    "error loading initial vesting file  {}",
                    err
                ))
            })?,
        )
        .map_err(|err| {
            ExecutionError::InitVestingError(format!(
                "error on deserialize initial vesting file  {}",
                err
            ))
        })?;

        let get_slot_at_timestamp = |config: &ExecutionConfig, timestamp: MassaTime| {
            massa_models::timeslots::get_latest_block_slot_at_timestamp(
                config.thread_count,
                config.t0,
                config.genesis_timestamp,
                timestamp,
            )
            .map_err(|e| {
                ExecutionError::InitVestingError(format!(
                    "can no get the slot at timestamp {} : {}",
                    timestamp, e
                ))
            })
        };

        for v in hashmap.values_mut() {
            if v.len().eq(&1) {
                return Err(ExecutionError::InitVestingError(
                    "vesting file should have more than one element".to_string(),
                ));
            } else {
                *v = v
                    .windows(2)
                    .map(|elements| {
                        let (mut prev, next) = (elements[0], elements[1]);

                        // retrieve the start_slot
                        if prev.timestamp.eq(&MassaTime::from(0)) {
                            // first range with timestamp = 0
                            prev.start_slot = Slot::min();
                        } else if let Some(slot) = get_slot_at_timestamp(config, prev.timestamp)? {
                            prev.start_slot = slot;
                        }

                        // retrieve the end_slot
                        if let Some(next_range_slot) =
                            get_slot_at_timestamp(config, next.timestamp)?
                        {
                            prev.end_slot = next_range_slot.get_prev_slot(config.thread_count)?;
                        }

                        Ok(prev)
                    })
                    // we don't need range with StartSlot(0,0) && EndSlot(0,0)
                    // this can happen when the timestamp is passed
                    .filter(|a| match a.as_ref() {
                        Ok(vesting) => {
                            !(vesting.end_slot == Slot::min() && vesting.start_slot == Slot::min())
                        }
                        Err(e) => {
                            error!(
                                "unable to retrieve reference for vesting range with error : {}",
                                e
                            );
                            false
                        }
                    })
                    .collect::<Result<Vec<VestingRange>, ExecutionError>>()?;
            }
        }

        Ok(hashmap)
    }

    /// find a vesting range in the registry, otherwise return None
    fn find_vesting_range(&self, addr: &Address, current_slot: &Slot) -> Option<&VestingRange> {
        let Some(vector) = self.vesting_registry.get(addr) else {
            return None;
        };

        vector.iter().find(|vesting| {
            vesting.start_slot <= *current_slot && vesting.end_slot >= *current_slot
        })
    }
}
