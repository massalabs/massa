// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module deals with executing final and active slots, as well as read-only requests.
//! It also keeps a history of executed slots, thus holding the speculative state of the ledger.
//!
//! Execution usually happens in the following way:
//! * an execution context is set up
//! * the VM is called for execution within this context
//! * the output of the execution is extracted from the context

use crate::active_history::{ActiveHistory, HistorySearchResult};
use crate::context::ExecutionContext;
use crate::interface_impl::InterfaceImpl;
use massa_async_pool::AsyncMessage;
use massa_execution_exports::{
    EventStore, ExecutionConfig, ExecutionError, ExecutionOutput, ExecutionStackElement,
    ReadOnlyExecutionRequest, ReadOnlyExecutionTarget,
};
use massa_final_state::FinalState;
use massa_ledger_exports::{SetOrDelete, SetUpdateOrDelete};
use massa_models::api::EventFilter;
use massa_models::output_event::SCOutputEvent;
use massa_models::{Address, BlockId, OperationId, OperationType, WrappedOperation};
use massa_models::{Amount, Slot};
use massa_sc_runtime::Interface;
use massa_storage::Storage;
use parking_lot::{Mutex, RwLock};
use std::collections::BTreeSet;
use std::{collections::HashMap, sync::Arc};
use tracing::debug;

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
    /// Shared storage across all modules
    storage: Storage,
}

impl ExecutionState {
    /// Create a new execution state. This should be called only once at the start of the execution worker.
    ///
    /// # Arguments
    /// * `config`: execution configuration
    /// * `final_state`: atomic access to the final state
    /// * `storage`: Shared storage with data shared all across the modules
    ///
    /// # returns
    /// A new `ExecutionState`
    pub fn new(
        config: ExecutionConfig,
        final_state: Arc<RwLock<FinalState>>,
        storage: Storage,
    ) -> ExecutionState {
        // Get the slot at the output of which the final state is attached.
        // This should be among the latest final slots.
        let last_final_slot = final_state.read().slot;

        // Create default active history
        let active_history: Arc<RwLock<ActiveHistory>> = Default::default();

        // Create an empty placeholder execution context, with shared atomic access
        let execution_context = Arc::new(Mutex::new(ExecutionContext::new(
            final_state.clone(),
            active_history.clone(),
        )));

        // Instantiate the interface providing ABI access to the VM, share the execution context with it
        let execution_interface = Box::new(InterfaceImpl::new(
            config.clone(),
            execution_context.clone(),
        ));

        // build the execution state
        ExecutionState {
            config,
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
            storage,
        }
    }

    /// Gets out the first (oldest) execution history item, removing it from history.
    ///
    /// # Returns
    /// The earliest `ExecutionOutput` from the execution history, or None if the history is empty
    pub fn pop_first_execution_result(&mut self) -> Option<ExecutionOutput> {
        self.active_history.write().0.pop_front()
    }

    /// Applies the output of an execution to the final execution state.
    /// The newly applied final output should be from the slot just after the last executed final slot
    ///
    /// # Arguments
    /// * `exec_ou`t: execution output to apply
    pub fn apply_final_execution_output(&mut self, exec_out: ExecutionOutput) {
        if self.final_cursor >= exec_out.slot {
            panic!("attempting to apply a final execution output at or before the current final_cursor");
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

    /// Clear the whole execution history,
    /// deleting caches on executed non-final slots.
    pub fn clear_history(&mut self) {
        // clear history
        self.active_history.write().0.clear();

        // reset active cursor to point to the latest final slot
        self.active_cursor = self.final_cursor;
    }

    /// This function receives a new sequence of blocks to execute as argument.
    /// It then scans the output history to see until which slot this sequence was already executed (and is outputs cached).
    /// If a mismatch is found, it means that the sequence of blocks to execute has changed
    /// and the existing output cache is truncated to keep output history only until the mismatch slot (excluded).
    /// Slots after that point will need to be (re-executed) to account for the new sequence.
    ///
    /// # Arguments
    /// * `active_slots`: A `HashMap` mapping each active slot to a block or None if the slot is a miss
    /// * `ready_final_slots`:  A `HashMap` mapping each ready-to-execute final slot to a block or None if the slot is a miss
    pub fn truncate_history(
        &mut self,
        active_slots: &HashMap<Slot, Option<BlockId>>,
        ready_final_slots: &HashMap<Slot, Option<BlockId>>,
    ) {
        // find mismatch point (included)
        let mut truncate_at = None;
        // iterate over the output history, in chronological order
        for (hist_index, exec_output) in self.active_history.read().0.iter().enumerate() {
            // try to find the corresponding slot in active_slots or ready_final_slots.
            let found_block_id = active_slots
                .get(&exec_output.slot)
                .or_else(|| ready_final_slots.get(&exec_output.slot));
            if found_block_id == Some(&exec_output.block_id) {
                // the slot number and block ID still match. Continue scanning
                continue;
            }
            // mismatch found: stop scanning and return the cutoff index
            truncate_at = Some(hist_index);
            break;
        }

        // If a mismatch was found
        if let Some(truncate_at) = truncate_at {
            // Truncate the execution output history at the cutoff index (excluded)
            self.active_history.write().0.truncate(truncate_at);
            // Now that part of the speculative executions were cancelled,
            // update the active cursor to match the latest executed slot.
            // The cursor is set to the latest executed final slot if the history is empty.
            self.active_cursor = self
                .active_history
                .read()
                .0
                .back()
                .map_or(self.final_cursor, |out| out.slot);
            // safety check to ensure that the active cursor cannot go too far back in time
            if self.active_cursor < self.final_cursor {
                panic!(
                    "active_cursor moved before final_cursor after execution history truncation"
                );
            }
        }
    }

    /// Execute an operation in the context of a block.
    /// Assumes the execution context was initialized at the beginning of the slot.
    ///
    /// # Arguments
    /// * `operation`: operation to execute
    /// * `block_creator_addr`: address of the block creator
    pub fn execute_operation(
        &self,
        operation: &WrappedOperation,
        block_creator_addr: Address,
    ) -> Result<(), ExecutionError> {
        // prefilter only SC operations
        match &operation.content.op {
            OperationType::ExecuteSC { .. } => {}
            OperationType::CallSC { .. } => {}
            _ => return Ok(()),
        };

        // call the execution process specific to the operation type
        match &operation.content.op {
            OperationType::ExecuteSC { .. } => self.execute_executesc_op(
                &operation.content.op,
                block_creator_addr,
                operation.id,
                operation.creator_address,
            ),
            OperationType::CallSC { .. } => self.execute_callsc_op(
                &operation.content.op,
                block_creator_addr,
                operation.id,
                operation.creator_address,
            ),
            _ => panic!("unexpected operation type"), // checked at the beginning of the function
        }
    }

    /// Execute an operation of type `ExecuteSC`
    /// Will panic if called with another operation type
    ///
    /// # Arguments
    /// * `operation`: the `WrappedOperation` to process, must be an `ExecuteSC`
    /// * `block_creator_addr`: address of the block creator
    /// * `operation_id`: ID of the operation
    /// * `sender_addr`: address of the sender
    pub fn execute_executesc_op(
        &self,
        operation: &OperationType,
        block_creator_addr: Address,
        operation_id: OperationId,
        sender_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process ExecuteSC operations only
        let (bytecode, max_gas, coins, gas_price) = match &operation {
            OperationType::ExecuteSC {
                data,
                max_gas,
                coins,
                gas_price,
            } => (data, max_gas, coins, gas_price),
            _ => panic!("unexpected operation type"),
        };

        // prepare the current slot context for executing the operation
        let context_snapshot;
        {
            // acquire write access to the context
            let mut context = context_guard!(self);

            // Use the context to credit the producer of the block with max_gas * gas_price parallel coins.
            // Note that errors are deterministic and do not cancel the operation execution.
            // That way, even if the sender sent an invalid operation, the block producer will still get credited.
            let gas_fees = gas_price.saturating_mul_u64(*max_gas);
            if let Err(err) =
                context.transfer_parallel_coins(None, Some(block_creator_addr), gas_fees)
            {
                debug!(
                    "failed to credit block producer {} with {} gas fee coins: {}",
                    block_creator_addr, gas_fees, err
                );
            }

            // Credit the operation sender with `coins` parallel coins.
            // Note that errors are deterministic and do not cancel op execution.
            if let Err(err) = context.transfer_parallel_coins(None, Some(sender_addr), *coins) {
                debug!(
                    "failed to credit operation sender {} with {} operation coins: {}",
                    sender_addr, *coins, err
                );
            }

            // save a snapshot of the context state to restore it if the op fails to execute,
            // this reverting any changes except the coin transfers above
            context_snapshot = context.get_snapshot();

            // set the context gas price to match the one defined in the operation
            context.gas_price = *gas_price;

            // set the context max gas to match the one defined in the operation
            context.max_gas = *max_gas;

            // Set the call stack to a single element:
            // * the execution will happen in the context of the address of the operation's sender
            // * the context will signal that `coins` were credited to the parallel balance of the sender during that call
            // * the context will give the operation's sender write access to its own ledger entry
            context.stack = vec![ExecutionStackElement {
                address: sender_addr,
                coins: *coins,
                owned_addresses: vec![sender_addr],
            }];

            // set the context origin operation ID
            context.origin_operation_id = Some(operation_id);
        };

        // run the VM on the bytecode contained in the operation
        let run_result = massa_sc_runtime::run_main(bytecode, *max_gas, &*self.execution_interface);
        if let Err(err) = run_result {
            // there was an error during bytecode execution:
            // cancel the effects of the execution by resetting the context to the previously saved snapshot
            let err = ExecutionError::RuntimeError(format!("bytecode execution error: {}", err));
            let mut context = context_guard!(self);
            context.reset_to_snapshot(context_snapshot, Some(err.clone()));
            context.origin_operation_id = None;
            return Err(err);
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
        block_creator_addr: Address,
        operation_id: OperationId,
        sender_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process CallSC operations only
        let (gas_price, max_gas, target_addr, target_func, param, parallel_coins, sequential_coins) =
            match &operation {
                OperationType::CallSC {
                    gas_price,
                    max_gas,
                    target_addr,
                    target_func,
                    param,
                    parallel_coins,
                    sequential_coins,
                } => (
                    *gas_price,
                    *max_gas,
                    *target_addr,
                    target_func,
                    param,
                    *parallel_coins,
                    *sequential_coins,
                ),
                _ => panic!("unexpected operation type"),
            };

        // prepare the current slot context for executing the operation
        let context_snapshot;
        let bytecode;
        {
            // acquire write access to the context
            let mut context = context_guard!(self);

            // Use the context to credit the producer of the block with max_gas * gas_price parallel coins.
            // Note that errors are deterministic and do not cancel the operation execution.
            // That way, even if the sender sent an invalid operation, the block producer will still get credited.
            let gas_fees = gas_price.saturating_mul_u64(max_gas);
            if let Err(err) =
                context.transfer_parallel_coins(None, Some(block_creator_addr), gas_fees)
            {
                debug!(
                    "failed to credit block producer {} with {} gas fee coins: {}",
                    block_creator_addr, gas_fees, err
                );
            }

            // Credit the operation sender with `sequential_coins` parallel coins.
            // This is used to ensure that those coins are not lost in case of failure,
            // since they have been debited by consensus beforehand.
            // Note that errors are deterministic and do not cancel op execution.
            if let Err(err) =
                context.transfer_parallel_coins(None, Some(sender_addr), sequential_coins)
            {
                debug!(
                    "failed to credit operation sender {} with {} operation coins: {}",
                    sender_addr, sequential_coins, err
                );
            }

            // Load bytecode. Assume empty bytecode if not found.
            bytecode = context.get_bytecode(&target_addr).unwrap_or_default();

            // save a snapshot of the context state to restore it if the op fails to execute,
            // thus reverting any changes except the coin transfers above
            context_snapshot = context.get_snapshot();

            // compute the total amount of coins that need to be transferred
            // from the sender's parallel balance to the target's parallel balance
            let coins = sequential_coins.saturating_add(parallel_coins);

            // set the context gas price to match the one defined in the operation
            context.gas_price = gas_price;

            // set the context max gas to match the one defined in the operation
            context.max_gas = max_gas;

            // set the context origin operation ID
            context.origin_operation_id = Some(operation_id);

            // Set the call stack o the sender addr only to allow it to send parallel coins (access rights)
            context.stack = vec![ExecutionStackElement {
                address: sender_addr,
                coins,
                owned_addresses: vec![sender_addr],
            }];

            // try to transfer parallel coins from the sender to the target
            let coin_transfer_result =
                context.transfer_parallel_coins(Some(sender_addr), Some(target_addr), coins);

            // Add the second part of the stack (the target)
            // Note that we do it here so that the sender is on the top of the stack for the coin transfer above.
            // and we don't do it after the coin transfer error handling because the emitted error event must show the correct stack.
            context.stack.push(ExecutionStackElement {
                address: target_addr,
                coins,
                owned_addresses: vec![target_addr],
            });

            // check the coin transfer result
            if let Err(err) = coin_transfer_result {
                // cancel the effects of the execution by resetting the context to the previously saved snapshot
                let err = ExecutionError::RuntimeError(format!(
                    "failed to transfer {} call coins from {} to {}: {}",
                    coins, sender_addr, target_addr, err
                ));
                context.reset_to_snapshot(context_snapshot, Some(err.clone()));
                context.origin_operation_id = None;
                return Err(err);
            }
        };

        // quit if there is no function to be called
        if target_func.is_empty() {
            return Ok(());
        }

        // run the VM on the called fucntion of the bytecode
        let run_result = massa_sc_runtime::run_function(
            &bytecode,
            max_gas,
            target_func,
            param,
            &*self.execution_interface,
        );
        if let Err(err) = run_result {
            // there was an error during bytecode execution:
            // cancel the effects of the execution by resetting the context to the previously saved snapshot
            let err = ExecutionError::RuntimeError(format!("bytecode execution error: {}", err));
            let mut context = context_guard!(self);
            context.reset_to_snapshot(context_snapshot, Some(err.clone()));
            context.origin_operation_id = None;
            return Err(err);
        }

        Ok(())
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
        bytecode: Option<Vec<u8>>,
    ) -> Result<(), ExecutionError> {
        // prepare execution context
        let context_snapshot;
        let (bytecode, data): (Vec<u8>, &str) = {
            let mut context = context_guard!(self);
            context_snapshot = context.get_snapshot();
            context.max_gas = message.max_gas;
            context.gas_price = message.gas_price;
            context.stack = vec![
                ExecutionStackElement {
                    address: message.sender,
                    coins: message.coins,
                    owned_addresses: vec![message.sender],
                },
                ExecutionStackElement {
                    address: message.destination,
                    coins: message.coins,
                    owned_addresses: vec![message.destination],
                },
            ];

            // If there is no target bytecode or if message data is invalid,
            // reimburse sender with coins and quit
            let (bytecode, data) = match (bytecode, std::str::from_utf8(&message.data)) {
                (Some(bc), Ok(d)) => (bc, d),
                (bc, _d) => {
                    let err = if bc.is_none() {
                        ExecutionError::RuntimeError("no target bytecode found".into())
                    } else {
                        ExecutionError::RuntimeError(
                            "message data does not convert to utf-8".into(),
                        )
                    };
                    context.reset_to_snapshot(context_snapshot, Some(err.clone()));
                    context.cancel_async_message(&message);
                    return Err(err);
                }
            };

            // credit coins to the target address
            if let Err(err) =
                context.transfer_parallel_coins(None, Some(message.destination), message.coins)
            {
                // coin crediting failed: reset context to snapshot and reimburse sender
                let err = ExecutionError::RuntimeError(format!(
                    "could not credit coins to target of async execution: {}",
                    err
                ));
                context.reset_to_snapshot(context_snapshot, Some(err.clone()));
                context.cancel_async_message(&message);
                return Err(err);
            }

            (bytecode, data)
        };

        // run the target function
        if let Err(err) = massa_sc_runtime::run_function(
            &bytecode,
            message.max_gas,
            &message.handler,
            data,
            &*self.execution_interface,
        ) {
            // execution failed: reset context to snapshot and reimburse sender
            let err = ExecutionError::RuntimeError(format!(
                "async message runtime execution error: {}",
                err
            ));
            let mut context = context_guard!(self);
            context.reset_to_snapshot(context_snapshot, Some(err.clone()));
            context.cancel_async_message(&message);
            Err(err)
        } else {
            Ok(())
        }
    }

    /// Executes a full slot (with or without a block inside) without causing any changes to the state,
    /// just yielding the execution output.
    ///
    /// # Arguments
    /// * `slot`: slot to execute
    /// * `opt_block`: block ID if there is a block a that slot, otherwise None
    ///
    /// # Returns
    /// An `ExecutionOutput` structure summarizing the output of the executed slot
    pub fn execute_slot(&self, slot: Slot, opt_block_id: Option<BlockId>) -> ExecutionOutput {
        // create a new execution context for the whole active slot
        let mut execution_context = ExecutionContext::active_slot(
            slot,
            opt_block_id,
            self.final_state.clone(),
            self.active_history.clone(),
        );

        // note that here, some pre-operations (like crediting block producers) can be performed before the lock

        // get asynchronous messages to execute
        let messages = execution_context.take_async_batch(self.config.max_async_gas);

        // apply the created execution context for slot execution
        *context_guard!(self) = execution_context;

        // Try executing asynchronous messages.
        // Effects are cancelled on failure and the sender is reimbursed.
        for (opt_bytecode, message) in messages {
            if let Err(err) = self.execute_async_message(message, opt_bytecode) {
                debug!("failed executing async message: {}", err);
            }
        }

        // check if there is a block at this slot
        if let Some(block_id) = opt_block_id {
            let block = self
                .storage
                .retrieve_block(&block_id)
                .expect("Missing block in storage.");
            let stored_block = block.read();
            // Try executing the operations of this block in the order in which they appear in the block.
            // Errors are logged but do not interrupt the execution of the slot.
            for (op_idx, operation) in stored_block.content.operations.iter().enumerate() {
                if let Err(err) =
                    self.execute_operation(operation, stored_block.content.header.creator_address)
                {
                    debug!(
                        "failed executing operation index {} in block {}: {}",
                        op_idx, block_id, err
                    );
                }
            }
        }

        // finish slot and return the execution output
        context_guard!(self).settle_slot()
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
    ) -> Result<ExecutionOutput, ExecutionError> {
        // set the execution slot to be the one after the latest executed active slot
        let slot = self
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow in readonly execution");

        // create a readonly execution context
        let execution_context = ExecutionContext::readonly(
            slot,
            req.max_gas,
            req.simulated_gas_price,
            req.call_stack,
            self.final_state.clone(),
            self.active_history.clone(),
        );

        // run the intepreter according to the target type
        match req.target {
            ReadOnlyExecutionTarget::BytecodeExecution(bytecode) => {
                // set the execution context for execution
                *context_guard!(self) = execution_context;

                // run the bytecode's main function
                massa_sc_runtime::run_main(&bytecode, req.max_gas, &*self.execution_interface)
                    .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;
            }
            ReadOnlyExecutionTarget::FunctionCall {
                target_addr,
                target_func,
                parameter,
            } => {
                // get the bytecode, default to an empty vector
                let bytecode = execution_context
                    .get_bytecode(&target_addr)
                    .unwrap_or_default();

                // set the execution context for execution
                *context_guard!(self) = execution_context;

                // run the target function in the bytecode
                massa_sc_runtime::run_function(
                    &bytecode,
                    req.max_gas,
                    &target_func,
                    &parameter,
                    &*self.execution_interface,
                )
                .map_err(|err| ExecutionError::RuntimeError(err.to_string()))?;
            }
        }

        // return the execution output
        Ok(context_guard!(self).settle_slot())
    }

    /// Gets a parallel balance both at the latest final and active executed slots
    pub fn get_final_and_active_parallel_balance(
        &self,
        address: &Address,
    ) -> (Option<Amount>, Option<Amount>) {
        let final_balance = self.final_state.read().ledger.get_parallel_balance(address);
        let search_result = self
            .active_history
            .read()
            .fetch_active_history_balance(address);
        (
            final_balance,
            match search_result {
                HistorySearchResult::Present(active_balance) => Some(active_balance),
                HistorySearchResult::NoInfo => final_balance,
                HistorySearchResult::Absent => None,
            },
        )
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
    pub fn get_final_and_active_datastore_keys(
        &self,
        addr: &Address,
    ) -> (BTreeSet<Vec<u8>>, BTreeSet<Vec<u8>>) {
        // here, get the final keys from the final ledger, and make a copy of it for the candidate list
        let final_keys = self.final_state.read().ledger.get_datastore_keys(addr);
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

    /// Gets execution events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    pub fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent> {
        self.final_events
            .get_filtered_sc_output_event(&filter)
            .into_iter()
            .chain(
                self.active_history
                    .read()
                    .0
                    .iter()
                    .flat_map(|item| item.events.get_filtered_sc_output_event(&filter)),
            )
            .collect()
    }
}
