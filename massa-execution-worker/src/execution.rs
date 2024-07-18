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
use crate::stats::ExecutionStatsCounter;
#[cfg(feature = "dump-block")]
use crate::storage_backend::StorageBackend;
use massa_async_pool::AsyncMessage;
use massa_execution_exports::{
    EventStore, ExecutedBlockInfo, ExecutionBlockMetadata, ExecutionChannels, ExecutionConfig,
    ExecutionError, ExecutionOutput, ExecutionQueryCycleInfos, ExecutionQueryStakerInfo,
    ExecutionStackElement, ReadOnlyExecutionOutput, ReadOnlyExecutionRequest,
    ReadOnlyExecutionTarget, SlotExecutionOutput,
};
use massa_final_state::FinalStateController;
use massa_ledger_exports::{SetOrDelete, SetUpdateOrDelete};
use massa_metrics::MassaMetrics;
use massa_models::address::ExecutionAddressCycleInfo;
use massa_models::bytecode::Bytecode;

use massa_models::datastore::get_prefix_bounds;
use massa_models::denunciation::{Denunciation, DenunciationIndex};
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashSet;
use massa_models::stats::ExecutionStats;
use massa_models::timeslots::get_block_slot_timestamp;
use massa_models::{
    address::Address,
    block_id::BlockId,
    operation::{OperationId, OperationType, SecureShareOperation},
};
use massa_models::{amount::Amount, slot::Slot};
use massa_module_cache::config::ModuleCacheConfig;
use massa_module_cache::controller::ModuleCache;
use massa_pos_exports::SelectorController;
use massa_sc_runtime::{Interface, Response, VMError};
use massa_versioning::versioning::MipStore;
use massa_wallet::Wallet;
use parking_lot::{Mutex, RwLock};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tracing::{debug, info, trace, warn};

use crate::execution_info::{AsyncMessageExecutionResult, DenunciationResult};
#[cfg(feature = "execution-info")]
use crate::execution_info::{ExecutionInfo, ExecutionInfoForSlot, OperationInfo};
#[cfg(feature = "execution-trace")]
use crate::trace_history::TraceHistory;
#[cfg(feature = "execution-trace")]
use massa_execution_exports::{AbiTrace, SlotAbiCallStack, Transfer};
#[cfg(feature = "dump-block")]
use massa_models::block::FilledBlock;
#[cfg(feature = "execution-trace")]
use massa_models::config::{BASE_OPERATION_GAS_COST, MAX_GAS_PER_BLOCK, MAX_OPERATIONS_PER_BLOCK};
#[cfg(feature = "dump-block")]
use massa_models::operation::Operation;
#[cfg(feature = "execution-trace")]
use massa_models::prehash::PreHashMap;
#[cfg(feature = "dump-block")]
use massa_models::secure_share::SecureShare;
#[cfg(feature = "dump-block")]
use massa_proto_rs::massa::model::v1 as grpc_model;
#[cfg(feature = "dump-block")]
use prost::Message;

/// Used to acquire a lock on the execution context
macro_rules! context_guard {
    ($self:ident) => {
        $self.execution_context.lock()
    };
}

#[cfg(feature = "execution-trace")]
/// ABI and execution succeed or not
pub type ExecutionResult = (Vec<AbiTrace>, bool);
#[cfg(not(feature = "execution-trace"))]
pub type ExecutionResult = ();

#[cfg(feature = "execution-trace")]
/// ABIs
pub type ExecutionResultInner = Vec<AbiTrace>;
#[cfg(not(feature = "execution-trace"))]
/// ABIs
pub type ExecutionResultInner = ();

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
    final_state: Arc<RwLock<dyn FinalStateController>>,
    // execution context (see documentation in context.rs)
    execution_context: Arc<Mutex<ExecutionContext>>,
    // execution interface allowing the VM runtime to access the Massa context
    execution_interface: Box<dyn Interface>,
    // execution statistics
    stats_counter: ExecutionStatsCounter,
    // cache of pre compiled sc modules
    module_cache: Arc<RwLock<ModuleCache>>,
    // MipStore (Versioning)
    mip_store: MipStore,
    // wallet used to verify double staking on local addresses
    wallet: Arc<RwLock<Wallet>>,
    // selector controller to get draws
    selector: Box<dyn SelectorController>,
    // channels used by the execution worker
    channels: ExecutionChannels,
    /// prometheus metrics
    massa_metrics: MassaMetrics,
    #[cfg(feature = "execution-trace")]
    pub(crate) trace_history: Arc<RwLock<TraceHistory>>,
    #[cfg(feature = "execution-info")]
    pub(crate) execution_info: Arc<RwLock<ExecutionInfo>>,
    #[cfg(feature = "dump-block")]
    block_storage_backend: Arc<RwLock<dyn StorageBackend>>,
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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: ExecutionConfig,
        final_state: Arc<RwLock<dyn FinalStateController>>,
        mip_store: MipStore,
        selector: Box<dyn SelectorController>,
        channels: ExecutionChannels,
        wallet: Arc<RwLock<Wallet>>,
        massa_metrics: MassaMetrics,
        #[cfg(feature = "dump-block")] block_storage_backend: Arc<RwLock<dyn StorageBackend>>,
    ) -> ExecutionState {
        // Get the slot at the output of which the final state is attached.
        // This should be among the latest final slots.
        let last_final_slot;
        let execution_trail_hash;
        {
            let final_state_read = final_state.read();
            last_final_slot = final_state_read.get_slot();
            execution_trail_hash = final_state_read.get_execution_trail_hash();
        }

        // Create default active history
        let active_history: Arc<RwLock<ActiveHistory>> = Default::default();

        // Initialize the SC module cache
        let module_cache = Arc::new(RwLock::new(ModuleCache::new(ModuleCacheConfig {
            hd_cache_path: config.hd_cache_path.clone(),
            gas_costs: config.gas_costs.clone(),
            lru_cache_size: config.lru_cache_size,
            hd_cache_size: config.hd_cache_size,
            snip_amount: config.snip_amount,
            max_module_length: config.max_bytecode_size,
        })));

        // Create an empty placeholder execution context, with shared atomic access
        let execution_context = Arc::new(Mutex::new(ExecutionContext::new(
            config.clone(),
            final_state.clone(),
            active_history.clone(),
            module_cache.clone(),
            mip_store.clone(),
            execution_trail_hash,
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
            mip_store,
            selector,
            channels,
            wallet,
            massa_metrics,
            #[cfg(feature = "execution-trace")]
            trace_history: Arc::new(RwLock::new(TraceHistory::new(
                config.max_execution_traces_slot_limit as u32,
                std::cmp::min(
                    MAX_OPERATIONS_PER_BLOCK,
                    (MAX_GAS_PER_BLOCK / BASE_OPERATION_GAS_COST) as u32,
                ),
            ))),
            #[cfg(feature = "execution-info")]
            execution_info: Arc::new(RwLock::new(ExecutionInfo::new(
                config.max_execution_traces_slot_limit as u32,
            ))),
            config,
            #[cfg(feature = "dump-block")]
            block_storage_backend,
        }
    }

    /// Get the fingerprint of the final state
    pub fn get_final_state_fingerprint(&self) -> massa_hash::Hash {
        self.final_state.read().get_fingerprint()
    }

    /// Get execution statistics
    pub fn get_stats(&self) -> ExecutionStats {
        self.stats_counter
            .get_stats(self.active_cursor, self.final_cursor)
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
        if exec_out.block_info.is_some() {
            self.stats_counter.register_final_blocks(1);
            self.stats_counter.register_final_executed_operations(
                exec_out.state_changes.executed_ops_changes.len(),
            );
            self.stats_counter.register_final_executed_denunciations(
                exec_out.state_changes.executed_denunciations_changes.len(),
            );
        }

        // Update versioning stats
        // This will update the MIP store and must be called before final state write
        // as it will also write the MIP store on disk
        self.update_versioning_stats(&exec_out.block_info, &exec_out.slot);

        let exec_out_2 = exec_out.clone();
        #[cfg(feature = "slot-replayer")]
        {
            println!(">>> Execution changes");
            println!("{:#?}", serde_json::to_string_pretty(&exec_out));
            println!("<<<");
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

        // update the prometheus metrics
        self.massa_metrics
            .set_active_cursor(self.active_cursor.period, self.active_cursor.thread);
        self.massa_metrics
            .set_final_cursor(self.final_cursor.period, self.final_cursor.thread);
        self.massa_metrics.inc_operations_final_counter(
            exec_out_2.state_changes.executed_ops_changes.len() as u64,
        );
        self.massa_metrics
            .set_active_history(self.active_history.read().0.len());

        self.massa_metrics
            .inc_sc_messages_final_by(exec_out_2.state_changes.async_pool_changes.0.len());

        self.massa_metrics.set_async_message_pool_size(
            self.final_state
                .read()
                .get_async_pool()
                .message_info_cache
                .len(),
        );

        self.massa_metrics.inc_executed_final_slot();
        if exec_out.block_info.is_some() {
            self.massa_metrics.inc_executed_final_slot_with_block();
        }

        // Broadcast a final slot execution output to active channel subscribers.
        if self.config.broadcast_enabled {
            let slot_exec_out = SlotExecutionOutput::FinalizedSlot(exec_out_2);
            if let Err(err) = self
                .channels
                .slot_execution_output_sender
                .send(slot_exec_out)
            {
                trace!(
                    "error, failed to broadcast final execution output for slot {} due to: {}",
                    exec_out.slot,
                    err
                );
            }
        }

        #[cfg(feature = "execution-trace")]
        {
            if self.config.broadcast_traces_enabled {
                if let Some((slot_trace, _)) = exec_out.slot_trace.clone() {
                    if let Err(err) = self
                        .channels
                        .slot_execution_traces_sender
                        .send((slot_trace, true))
                    {
                        trace!(
                            "error, failed to broadcast abi trace for slot {} due to: {}",
                            exec_out.slot.clone(),
                            err
                        );
                    }
                }
            }
        }

        #[cfg(feature = "dump-block")]
        {
            let mut block_ser = vec![];
            if let Some(block_info) = exec_out.block_info {
                let block_id = block_info.block_id;
                let storage = exec_out.storage.unwrap();
                let guard = storage.read_blocks();
                let secured_block = guard
                    .get(&block_id)
                    .unwrap_or_else(|| panic!("Unable to get block for block id: {}", block_id));

                let operations: Vec<(OperationId, Option<SecureShare<Operation, OperationId>>)> =
                    secured_block
                        .content
                        .operations
                        .iter()
                        .map(|operation_id| {
                            match storage.read_operations().get(operation_id).cloned() {
                                Some(verifiable_operation) => {
                                    (*operation_id, Some(verifiable_operation))
                                }
                                None => (*operation_id, None),
                            }
                        })
                        .collect();

                let filled_block = FilledBlock {
                    header: secured_block.content.header.clone(),
                    operations,
                };

                let grpc_filled_block = grpc_model::FilledBlock::from(filled_block);
                grpc_filled_block.encode(&mut block_ser).unwrap();
            }

            self.block_storage_backend
                .write()
                .write(&exec_out.slot, &block_ser);
        }
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

        // update the prometheus metrics
        self.massa_metrics
            .set_active_history(self.active_history.read().0.len())
    }

    /// Helper function.
    /// Within a locked execution context (lock is taken at the beginning of the function then released at the end):
    /// - if not yet executed then transfer fee and add the operation to the context then return a context snapshot
    ///
    /// # Arguments
    /// * `operation`: operation to be schedule
    /// * `sender_addr`: sender address for the operation (for fee transfer)
    fn prepare_operation_for_execution(
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

        // Compute the minimal amount of coins the sender is allowed to have after the execution of this op based on `op.max_spending`.
        // Note that the max spending might exceed the sender's balance.
        let creator_initial_balance = context
            .get_balance(&sender_addr)
            .unwrap_or_else(Amount::zero);
        context.creator_min_balance = Some(
            creator_initial_balance
                .saturating_sub(operation.get_max_spending(self.config.roll_price)),
        );

        // debit the fee from the operation sender
        if let Err(err) =
            context.transfer_coins(Some(sender_addr), None, operation.content.fee, false)
        {
            let error = format!("could not spend fees: {}", err);
            let event = context.event_create(error.clone(), true);
            context.event_emit(event);
            return Err(ExecutionError::IncludeOperationError(error));
        }

        // from here, fees have been transferred.
        // Op will be executed just after in the context of a snapshot.

        // save a snapshot of the context to revert any further changes on error
        let context_snapshot = context.get_snapshot();

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
    ) -> Result<ExecutionResult, ExecutionError> {
        // check validity period
        if !(operation
            .get_validity_range(self.config.operation_validity_period)
            .contains(&block_slot.period))
        {
            return Err(ExecutionError::InvalidSlotRange);
        }

        // check remaining block gas
        let op_gas = operation.get_gas_usage(
            self.config.base_operation_gas_cost,
            self.config.gas_costs.sp_compilation_cost,
        );
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

        let context_snapshot = self.prepare_operation_for_execution(operation, sender_addr)?;

        // update block gas
        *remaining_block_gas = new_remaining_block_gas;

        // update block credits
        *block_credits = new_block_credits;

        #[cfg(feature = "execution-trace")]
        let res = vec![];
        #[allow(clippy::let_unit_value)]
        #[cfg(not(feature = "execution-trace"))]
        let res = ();
        // Call the execution process specific to the operation type.
        let mut execution_result = match &operation.content.op {
            OperationType::ExecuteSC { .. } => {
                self.execute_executesc_op(&operation.content.op, sender_addr)
            }
            OperationType::CallSC { .. } => {
                self.execute_callsc_op(&operation.content.op, sender_addr)
            }
            OperationType::RollBuy { .. } => self
                .execute_roll_buy_op(&operation.content.op, sender_addr)
                .map(|_| res),
            OperationType::RollSell { .. } => self
                .execute_roll_sell_op(&operation.content.op, sender_addr)
                .map(|_| res),
            OperationType::Transaction { .. } => self
                .execute_transaction_op(&operation.content.op, sender_addr)
                .map(|_| res),
        };

        {
            // lock execution context
            let mut context = context_guard!(self);

            if execution_result.is_ok() {
                // check that the `max_coins` spending limit was respected by the sender
                if let Some(creator_min_balance) = &context.creator_min_balance {
                    let creator_balance = context
                        .get_balance(&sender_addr)
                        .unwrap_or_else(Amount::zero);
                    if &creator_balance < creator_min_balance {
                        execution_result = Err(ExecutionError::RuntimeError(format!(
                            "at the end of the execution of the operation, the sender {} was expected to have at least {} coins according to the operation's max spending, but has only {}.",
                            sender_addr, creator_min_balance, creator_balance
                        )));
                    }
                }
            }

            // check execution results
            match execution_result {
                Ok(_value) => {
                    context.insert_executed_op(
                        operation_id,
                        true,
                        Slot::new(operation.content.expire_period, op_thread),
                    );
                    #[cfg(feature = "execution-trace")]
                    {
                        Ok((_value, true))
                    }
                    #[cfg(not(feature = "execution-trace"))]
                    {
                        Ok(())
                    }
                }
                Err(err) => {
                    // an error occurred: emit error event and reset context to snapshot
                    let err = ExecutionError::RuntimeError(format!(
                        "runtime error when executing operation {}: {}",
                        operation_id, &err
                    ));
                    debug!("{}", &err);
                    context.reset_to_snapshot(context_snapshot, err);

                    // Insert op AFTER the context has been restored (otherwise it would be overwritten)
                    context.insert_executed_op(
                        operation_id,
                        false,
                        Slot::new(operation.content.expire_period, op_thread),
                    );
                    #[cfg(feature = "execution-trace")]
                    {
                        Ok((vec![], false))
                    }
                    #[cfg(not(feature = "execution-trace"))]
                    {
                        Ok(())
                    }
                }
            }
        }
    }

    /// Execute a denunciation in the context of a block.
    ///
    /// # Arguments
    /// * `denunciation`: denunciation to process
    /// * `block_credits`: mutable reference towards the total block reward/fee credits
    fn execute_denunciation(
        &self,
        denunciation: &Denunciation,
        block_slot: &Slot,
        block_credits: &mut Amount,
    ) -> Result<DenunciationResult, ExecutionError> {
        let addr_denounced = Address::from_public_key(denunciation.get_public_key());

        // acquire write access to the context
        let mut context = context_guard!(self);

        let de_slot = denunciation.get_slot();

        if de_slot.period <= self.config.last_start_period {
            // denunciation created before last restart (can be 0 or >= 0 after a network restart) - ignored
            // Note: as we use '<=', also ignore denunciation created for genesis block
            return Err(ExecutionError::IncludeDenunciationError(format!(
                "Denunciation target ({}) is before the last start period: {}",
                de_slot, self.config.last_start_period
            )));
        }

        // ignore denunciation if not valid
        if !denunciation.is_valid() {
            return Err(ExecutionError::IncludeDenunciationError(
                "denunciation is not valid".to_string(),
            ));
        }

        // ignore denunciation if too old or expired

        if Denunciation::is_expired(
            &de_slot.period,
            &block_slot.period,
            &self.config.denunciation_expire_periods,
        ) {
            // too old - cannot be denounced anymore
            return Err(ExecutionError::IncludeDenunciationError(format!(
                "Denunciation target ({}) is too old with respect to the block ({})",
                de_slot, block_slot
            )));
        }

        if de_slot > block_slot {
            // too much in the future - ignored
            // Note: de_slot == block_slot is OK,
            //       for example if the block producer wants to denounce someone who multi-endorsed
            //       for the block's slot
            return Err(ExecutionError::IncludeDenunciationError(format!(
                "Denunciation target ({}) is at a later slot than the block slot ({})",
                de_slot, block_slot
            )));
        }

        // ignore the denunciation if it was already executed
        let de_idx = DenunciationIndex::from(denunciation);
        if context.is_denunciation_executed(&de_idx) {
            return Err(ExecutionError::IncludeDenunciationError(
                "Denunciation was already executed".to_string(),
            ));
        }

        // Check selector
        // Note 1: Has to be done after slot limit and executed check
        // Note 2: that this is done for a node to create a Block with 'fake' denunciation thus
        //       include them in executed denunciation and prevent (by occupying the corresponding entry)
        //       any further 'real' denunciation.

        match &denunciation {
            Denunciation::Endorsement(_de) => {
                // Get selected address from selector and check
                let selection = self
                    .selector
                    .get_selection(*de_slot)
                    .expect("Could not get producer from selector");
                let selected_addr = selection
                    .endorsements
                    .get(*denunciation.get_index().unwrap_or(&0) as usize)
                    .expect("could not get selection for endorsement at index");

                if *selected_addr != addr_denounced {
                    return Err(ExecutionError::IncludeDenunciationError(
                        "Attempt to execute a denunciation but address was not selected"
                            .to_string(),
                    ));
                }
            }
            Denunciation::BlockHeader(_de) => {
                let selected_addr = self
                    .selector
                    .get_producer(*de_slot)
                    .expect("Cannot get producer from selector");

                if selected_addr != addr_denounced {
                    return Err(ExecutionError::IncludeDenunciationError(
                        "Attempt to execute a denunciation but address was not selected"
                            .to_string(),
                    ));
                }
            }
        }

        context.insert_executed_denunciation(&de_idx);

        let slashed = context.try_slash_rolls(
            &addr_denounced,
            self.config.roll_count_to_slash_on_denunciation,
        );

        match slashed.as_ref() {
            Ok(slashed_amount) => {
                // Add slashed amount / 2 to block reward
                let amount = slashed_amount.checked_div_u64(2).ok_or_else(|| {
                    ExecutionError::RuntimeError(format!(
                        "Unable to divide slashed amount: {} by 2",
                        slashed_amount
                    ))
                })?;
                *block_credits = block_credits.saturating_add(amount);
            }
            Err(e) => {
                warn!("Unable to slash rolls or deferred credits: {}", e);
            }
        }

        if self
            .wallet
            .read()
            .get_wallet_address_list()
            .contains(&addr_denounced)
        {
            match &denunciation.is_for_block_header() {
                true => panic!("You are being slashed at slot {} for double-staking using address {}. The node is stopping to prevent any further loss. Block header denunciation of block at slot {:?}. Denunciation's public key: {:?}", block_slot, addr_denounced, denunciation.get_slot(), denunciation.get_public_key()),
                false => panic!("You are being slashed at slot {} for double-staking using address {}. The node is stopping to prevent any further loss. Endorsement denunciation of endorsement at slot {:?} and index {:?}. Denunciation's public key: {:?}", block_slot, addr_denounced, denunciation.get_slot(), denunciation.get_index(), denunciation.get_public_key())
            }
        }

        Ok(DenunciationResult {
            address_denounced: addr_denounced,
            slot: *de_slot,
            slashed: slashed.unwrap_or_default(),
        })
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
    /// * `buyer_addr`: address of the buyer
    pub fn execute_roll_buy_op(
        &self,
        operation: &OperationType,
        buyer_addr: Address,
    ) -> Result<(), ExecutionError> {
        // process roll buy operations only
        let roll_count = match operation {
            OperationType::RollBuy { roll_count } => roll_count,
            _ => panic!("unexpected operation type"),
        };

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

        // add rolls to the buyer within the context
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

        // transfer coins from sender to destination
        if let Err(err) =
            context.transfer_coins(Some(sender_addr), Some(*recipient_address), *amount, true)
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
    ) -> Result<ExecutionResultInner, ExecutionError> {
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
            // This needs to be defined before anything can fail, so that the emitted event
            // contains the right stack
            context.stack = vec![ExecutionStackElement {
                address: sender_addr,
                coins: Amount::zero(),
                owned_addresses: vec![sender_addr],
                operation_datastore: Some(datastore.clone()),
            }];
        };

        // load the tmp module
        let module = self
            .module_cache
            .read()
            .load_tmp_module(bytecode, *max_gas)?;
        // run the VM
        let _res = massa_sc_runtime::run_main(
            &*self.execution_interface,
            module,
            *max_gas,
            self.config.gas_costs.clone(),
        )
        .map_err(|error| ExecutionError::VMError {
            context: "ExecuteSC".to_string(),
            error,
        })?;

        #[cfg(feature = "execution-trace")]
        {
            Ok(_res.trace.into_iter().map(|t| t.into()).collect())
        }
        #[cfg(not(feature = "execution-trace"))]
        {
            Ok(())
        }
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
    ) -> Result<ExecutionResultInner, ExecutionError> {
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

            // Ensure that the target address is an SC address
            // Ensure that the target address exists
            context.check_target_sc_address(target_addr)?;

            // Transfer coins from the sender to the target
            if let Err(err) =
                context.transfer_coins(Some(sender_addr), Some(target_addr), coins, false)
            {
                return Err(ExecutionError::RuntimeError(format!(
                    "failed to transfer {} operation coins from {} to {}: {}",
                    coins, sender_addr, target_addr, err
                )));
            }

            // quit if there is no function to be called
            if target_func.is_empty() {
                return Err(ExecutionError::RuntimeError(
                    "no function to call in the CallSC operation".to_string(),
                ));
            }

            // Load bytecode. Assume empty bytecode if not found.
            bytecode = context.get_bytecode(&target_addr).unwrap_or_default().0;
        }

        // load and execute the compiled module
        // IMPORTANT: do not keep a lock here as `run_function` uses the `get_module` interface
        let module = self.module_cache.write().load_module(&bytecode, max_gas)?;
        let response = massa_sc_runtime::run_function(
            &*self.execution_interface,
            module,
            target_func,
            param,
            max_gas,
            self.config.gas_costs.clone(),
        );
        match response {
            Ok(Response { init_gas_cost, .. })
            | Err(VMError::ExecutionError { init_gas_cost, .. }) => {
                self.module_cache
                    .write()
                    .set_init_cost(&bytecode, init_gas_cost);
            }
            _ => (),
        }
        let _response = response.map_err(|error| ExecutionError::VMError {
            context: "CallSC".to_string(),
            error,
        })?;
        #[cfg(feature = "execution-trace")]
        {
            Ok(_response.trace.into_iter().map(|t| t.into()).collect())
        }
        #[cfg(not(feature = "execution-trace"))]
        {
            Ok(())
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
        execution_version: u32,
    ) -> Result<AsyncMessageExecutionResult, ExecutionError> {
        let mut result = AsyncMessageExecutionResult::new();
        #[cfg(feature = "execution-info")]
        {
            // TODO: From impl + no ::new -> no cfg feature
            result.sender = Some(message.sender);
            result.destination = Some(message.destination);
        }

        // prepare execution context
        let context_snapshot;
        let bytecode = {
            let mut context = context_guard!(self);
            context_snapshot = context.get_snapshot();
            context.creator_address = None;
            context.creator_min_balance = None;
            context.stack = vec![
                ExecutionStackElement {
                    address: message.sender,
                    coins: match execution_version {
                        0 => message.coins,
                        _ => Default::default(),
                    },
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

            // check the target address
            if let Err(err) = context.check_target_sc_address(message.destination) {
                context.reset_to_snapshot(context_snapshot, err.clone());
                context.cancel_async_message(&message);
                return Err(err);
            }

            // if there is no bytecode: fail
            let bytecode = match bytecode {
                Some(bytecode) => bytecode,
                None => {
                    let err = ExecutionError::RuntimeError("no target bytecode found".into());
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
            } else {
                result.coins = Some(message.coins);
            }

            bytecode.0
        };

        // load and execute the compiled module
        // IMPORTANT: do not keep a lock here as `run_function` uses the `get_module` interface
        let module = match context_guard!(self).execution_component_version {
            0 => self
                .module_cache
                .write()
                .load_module(&bytecode, message.max_gas)?,
            _ => {
                let Ok(_module) = self
                    .module_cache
                    .write()
                    .load_module(&bytecode, message.max_gas)
                else {
                    let err = ExecutionError::RuntimeError(
                        "could not load module for async execution".into(),
                    );
                    let mut context = context_guard!(self);
                    context.reset_to_snapshot(context_snapshot, err.clone());
                    context.cancel_async_message(&message);
                    return Err(err);
                };
                _module
            }
        };

        let response = massa_sc_runtime::run_function(
            &*self.execution_interface,
            module,
            &message.function,
            &message.function_params,
            message.max_gas,
            self.config.gas_costs.clone(),
        );
        match response {
            Ok(res) => {
                self.module_cache
                    .write()
                    .set_init_cost(&bytecode, res.init_gas_cost);
                #[cfg(feature = "execution-trace")]
                {
                    result.traces = Some((res.trace.into_iter().map(|t| t.into()).collect(), true));
                }
                #[cfg(feature = "execution-info")]
                {
                    result.success = true;
                }
                Ok(result)
            }
            Err(error) => {
                if let VMError::ExecutionError { init_gas_cost, .. } = error {
                    self.module_cache
                        .write()
                        .set_init_cost(&bytecode, init_gas_cost);
                }
                // execution failed: reset context to snapshot and reimburse sender
                let err = ExecutionError::VMError {
                    context: "Asynchronous Message".to_string(),
                    error,
                };
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
    /// * `exec_target`: metadata of the block to execute, if not miss
    /// * `selector`: Reference to the selector
    ///
    /// # Returns
    /// An `ExecutionOutput` structure summarizing the output of the executed slot
    pub fn execute_slot(
        &self,
        slot: &Slot,
        exec_target: Option<&(BlockId, ExecutionBlockMetadata)>,
        selector: Box<dyn SelectorController>,
    ) -> ExecutionOutput {
        #[cfg(feature = "execution-trace")]
        let mut slot_trace = SlotAbiCallStack {
            slot: *slot,
            operation_call_stacks: PreHashMap::default(),
            asc_call_stacks: vec![],
        };
        #[cfg(feature = "execution-trace")]
        let mut transfers = vec![];

        #[cfg(feature = "execution-info")]
        let mut exec_info = ExecutionInfoForSlot::new();

        // Create a new execution context for the whole active slot
        let mut execution_context = ExecutionContext::active_slot(
            self.config.clone(),
            *slot,
            exec_target.as_ref().map(|(b_id, _)| *b_id),
            self.final_state.clone(),
            self.active_history.clone(),
            self.module_cache.clone(),
            self.mip_store.clone(),
        );

        let execution_version = execution_context.execution_component_version;
        match execution_version {
            0 => {
                // Get asynchronous messages to execute
                let messages = execution_context.take_async_batch_v0(
                    self.config.max_async_gas,
                    self.config.async_msg_cst_gas_cost,
                );

                // Apply the created execution context for slot execution
                *context_guard!(self) = execution_context;

                // Try executing asynchronous messages.
                // Effects are cancelled on failure and the sender is reimbursed.
                for (opt_bytecode, message) in messages {
                    match self.execute_async_message(message, opt_bytecode, execution_version) {
                        Ok(_message_return) => {
                            cfg_if::cfg_if! {
                                if #[cfg(feature = "execution-trace")] {
                                    // Safe to unwrap
                                    slot_trace.asc_call_stacks.push(_message_return.traces.unwrap().0);
                                } else if #[cfg(feature = "execution-info")] {
                                    slot_trace.asc_call_stacks.push(_message_return.traces.clone().unwrap().0);
                                    exec_info.async_messages.push(Ok(_message_return));
                                }
                            }
                        }
                        Err(err) => {
                            let msg = format!("failed executing async message: {}", err);
                            #[cfg(feature = "execution-info")]
                            exec_info.async_messages.push(Err(msg.clone()));
                            debug!(msg);
                        }
                    }
                }
            }
            _ => {
                // Get asynchronous messages to execute
                let messages = execution_context.take_async_batch_v1(
                    self.config.max_async_gas,
                    self.config.async_msg_cst_gas_cost,
                );

                // Apply the created execution context for slot execution
                *context_guard!(self) = execution_context;

                // Try executing asynchronous messages.
                // Effects are cancelled on failure and the sender is reimbursed.
                for (_message_id, message) in messages {
                    let opt_bytecode = context_guard!(self).get_bytecode(&message.destination);

                    match self.execute_async_message(message, opt_bytecode, execution_version) {
                        Ok(_message_return) => {
                            cfg_if::cfg_if! {
                                if #[cfg(feature = "execution-trace")] {
                                    // Safe to unwrap
                                    slot_trace.asc_call_stacks.push(_message_return.traces.unwrap().0);
                                } else if #[cfg(feature = "execution-info")] {
                                    slot_trace.asc_call_stacks.push(_message_return.traces.clone().unwrap().0);
                                    exec_info.async_messages.push(Ok(_message_return));
                                }
                            }
                        }
                        Err(err) => {
                            let msg = format!("failed executing async message: {}", err);
                            #[cfg(feature = "execution-info")]
                            exec_info.async_messages.push(Err(msg.clone()));
                            debug!(msg);
                        }
                    }
                }
            }
        }

        let mut block_info: Option<ExecutedBlockInfo> = None;

        // Check if there is a block at this slot
        if let Some((block_id, block_metadata)) = exec_target {
            let block_store = block_metadata
                .storage
                .as_ref()
                .expect("Cannot execute a block for which the storage is missing");

            // Retrieve the block from storage
            let stored_block = block_store
                .read_blocks()
                .get(block_id)
                .expect("Missing block in storage.")
                .clone();

            block_info = Some(ExecutedBlockInfo {
                block_id: *block_id,
                current_version: stored_block.content.header.content.current_version,
                announced_version: stored_block.content.header.content.announced_version,
            });

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
            let endorsement_creators: Vec<Address> = stored_block
                .content
                .header
                .content
                .endorsements
                .iter()
                .map(|endo| endo.content_creator_address)
                .collect();
            let endorsement_target_creator = block_metadata
                .same_thread_parent_creator
                .expect("same thread parent creator missing");

            // Set remaining block gas
            let mut remaining_block_gas = self.config.max_gas_per_block;

            // Set block credits
            let mut block_credits = self.config.block_reward;

            // Try executing the operations of this block in the order in which they appear in the block.
            // Errors are logged but do not interrupt the execution of the slot.
            for operation in operations.into_iter() {
                match self.execute_operation(
                    &operation,
                    stored_block.content.header.content.slot,
                    &mut remaining_block_gas,
                    &mut block_credits,
                ) {
                    Ok(_op_return) => {
                        #[cfg(feature = "execution-trace")]
                        {
                            slot_trace
                                .operation_call_stacks
                                .insert(operation.id, _op_return.0);
                            match &operation.content.op {
                                OperationType::Transaction {
                                    recipient_address,
                                    amount,
                                } => {
                                    let receiver_balance = {
                                        let context = context_guard!(self);
                                        context.get_balance(recipient_address).unwrap_or_default()
                                    };
                                    let mut effective_received_amount = *amount;
                                    if receiver_balance
                                        == amount
                                            .checked_sub(
                                                self.config
                                                    .storage_costs_constants
                                                    .ledger_entry_base_cost,
                                            )
                                            .unwrap_or_default()
                                    {
                                        effective_received_amount = amount
                                            .checked_sub(
                                                self.config
                                                    .storage_costs_constants
                                                    .ledger_entry_base_cost,
                                            )
                                            .unwrap_or_default();
                                    }
                                    transfers.push(Transfer {
                                        from: operation.content_creator_address,
                                        to: *recipient_address,
                                        amount: *amount,
                                        effective_received_amount,
                                        op_id: operation.id,
                                        succeed: _op_return.1,
                                        fee: operation.content.fee,
                                    });
                                }
                                OperationType::CallSC {
                                    target_addr, coins, ..
                                } => {
                                    transfers.push(Transfer {
                                        from: operation.content_creator_address,
                                        to: *target_addr,
                                        amount: *coins,
                                        effective_received_amount: *coins,
                                        op_id: operation.id,
                                        succeed: _op_return.1,
                                        fee: operation.content.fee,
                                    });
                                }
                                _ => {}
                            }
                        }

                        #[cfg(feature = "execution-info")]
                        {
                            match &operation.content.op {
                                OperationType::RollBuy { roll_count } => exec_info
                                    .operations
                                    .push(OperationInfo::RollBuy(*roll_count)),
                                OperationType::RollSell { roll_count } => exec_info
                                    .operations
                                    .push(OperationInfo::RollSell(*roll_count)),
                                _ => {}
                            }
                        }
                    }
                    Err(err) => {
                        debug!(
                            "failed executing operation {} in block {}: {}",
                            operation.id, block_id, err
                        );
                    }
                }
            }

            // Try executing the denunciations of this block
            for denunciation in &stored_block.content.header.content.denunciations {
                match self.execute_denunciation(
                    denunciation,
                    &stored_block.content.header.content.slot,
                    &mut block_credits,
                ) {
                    Ok(_de_res) => {
                        #[cfg(feature = "execution-info")]
                        exec_info.denunciations.push(Ok(_de_res));
                    }
                    Err(e) => {
                        let msg = format!(
                            "Failed processing denunciation: {:?}, in block: {}: {}",
                            denunciation, block_id, e
                        );
                        #[cfg(feature = "execution-info")]
                        exec_info.denunciations.push(Err(msg.clone()));
                        debug!(msg);
                    }
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
            for endorsement_creator in endorsement_creators {
                // credit creator of the endorsement with coins
                match context.transfer_coins(
                    None,
                    Some(endorsement_creator),
                    block_credit_part,
                    false,
                ) {
                    Ok(_) => {
                        remaining_credit = remaining_credit.saturating_sub(block_credit_part);

                        #[cfg(feature = "execution-info")]
                        exec_info
                            .endorsement_creator_rewards
                            .insert(endorsement_creator, block_credit_part);
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
                        #[cfg(feature = "execution-info")]
                        {
                            exec_info.endorsement_target_reward =
                                Some((endorsement_target_creator, block_credit_part));
                        }
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
            } else {
                #[cfg(feature = "execution-info")]
                {
                    exec_info.block_producer_reward = Some((block_creator_addr, remaining_credit));
                }
            }
        } else {
            // the slot is a miss, check who was supposed to be the creator and update production stats
            let producer_addr = selector
                .get_producer(*slot)
                .expect("couldn't get the expected block producer for a missed slot");
            context_guard!(self).update_production_stats(&producer_addr, *slot, None);
        }

        #[cfg(feature = "execution-trace")]
        self.trace_history
            .write()
            .save_traces_for_slot(*slot, slot_trace.clone());
        #[cfg(feature = "execution-trace")]
        self.trace_history
            .write()
            .save_transfers_for_slot(*slot, transfers.clone());

        // Finish slot
        #[allow(unused_mut)]
        let mut exec_out = context_guard!(self).settle_slot(block_info);
        #[cfg(feature = "execution-trace")]
        {
            exec_out.slot_trace = Some((slot_trace, transfers));
        };
        #[cfg(feature = "dump-block")]
        {
            exec_out.storage = match exec_target {
                Some((_block_id, block_metadata)) => block_metadata.storage.clone(),
                _ => None,
            }
        }

        #[cfg(feature = "execution-info")]
        {
            exec_info.deferred_credits_execution =
                std::mem::replace(&mut exec_out.deferred_credits_execution, vec![]);
            exec_info.cancel_async_message_execution =
                std::mem::replace(&mut exec_out.cancel_async_message_execution, vec![]);
            exec_info.auto_sell_execution =
                std::mem::replace(&mut exec_out.auto_sell_execution, vec![]);
            self.execution_info.write().save_for_slot(*slot, exec_info);
        }

        // Broadcast a slot execution output to active channel subscribers.
        if self.config.broadcast_enabled {
            let slot_exec_out = SlotExecutionOutput::ExecutedSlot(exec_out.clone());
            if let Err(err) = self
                .channels
                .slot_execution_output_sender
                .send(slot_exec_out)
            {
                trace!(
                    "error, failed to broadcast execution output for slot {} due to: {}",
                    exec_out.slot.clone(),
                    err
                );
            }
        }

        // Return the execution output
        exec_out
    }

    /// Execute a candidate slot
    pub fn execute_candidate_slot(
        &mut self,
        slot: &Slot,
        exec_target: Option<&(BlockId, ExecutionBlockMetadata)>,
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

        #[cfg(feature = "execution-trace")]
        {
            if self.config.broadcast_traces_enabled {
                if let Some((slot_trace, _)) = exec_out.slot_trace.clone() {
                    if let Err(err) = self
                        .channels
                        .slot_execution_traces_sender
                        .send((slot_trace, false))
                    {
                        trace!(
                            "error, failed to broadcast abi trace for slot {} due to: {}",
                            exec_out.slot.clone(),
                            err
                        );
                    }
                }
            }
        }

        // apply execution output to active state
        self.apply_active_execution_output(exec_out);

        debug!("execute_candidate_slot: execution finished & state applied");
    }

    /// Execute an SCE-final slot
    pub fn execute_final_slot(
        &mut self,
        slot: &Slot,
        exec_target: Option<&(BlockId, ExecutionBlockMetadata)>,
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
            if &exec_out.slot == slot
                && exec_out.block_info.as_ref().map(|i| i.block_id) == target_id
            {
                // speculative execution front result matches what we want to compute
                // apply the cached output and return
                self.apply_final_execution_output(exec_out);
                return;
            } else {
                // speculative cache mismatch
                warn!(
                    "speculative execution cache mismatch (final slot={}/block={:?}, front speculative slot={}/block={:?}). Resetting the cache.",
                    slot, target_id, exec_out.slot, exec_out.block_info.map(|i| i.block_id)
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
        let exec_out = self.execute_slot(slot, exec_target, selector);

        // apply execution output to final state
        self.apply_final_execution_output(exec_out);

        debug!(
            "execute_final_slot: execution finished & result applied & versioning stats updated"
        );
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

        // set the execution slot to be the one after the latest executed active slot
        let slot = self
            .active_cursor
            .get_next_slot(self.config.thread_count)
            .expect("slot overflow in readonly execution from active slot");

        // create a readonly execution context
        let execution_context = ExecutionContext::readonly(
            self.config.clone(),
            slot,
            req.call_stack,
            self.final_state.clone(),
            self.active_history.clone(),
            self.module_cache.clone(),
            self.mip_store.clone(),
        );

        // run the interpreter according to the target type
        let exec_response = match req.target {
            ReadOnlyExecutionTarget::BytecodeExecution(bytecode) => {
                {
                    let mut context = context_guard!(self);
                    *context = execution_context;

                    let call_stack_addr = context.get_call_stack();

                    // transfer fee
                    if let (Some(fee), Some(addr)) = (req.fee, call_stack_addr.get(0)) {
                        context.transfer_coins(Some(*addr), None, fee, false)?;
                    }
                }

                // load the tmp module
                let module = self
                    .module_cache
                    .read()
                    .load_tmp_module(&bytecode, req.max_gas)?;

                // run the VM
                massa_sc_runtime::run_main(
                    &*self.execution_interface,
                    module,
                    req.max_gas,
                    self.config.gas_costs.clone(),
                )
                .map_err(|error| ExecutionError::VMError {
                    context: "ReadOnlyExecutionTarget::BytecodeExecution".to_string(),
                    error,
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

                {
                    let mut context = context_guard!(self);
                    *context = execution_context;

                    // Ensure that the target address is an SC address and exists
                    context.check_target_sc_address(target_addr)?;

                    let call_stack_addr = context.get_call_stack();

                    // transfer fee
                    if let (Some(fee), Some(addr)) = (req.fee, call_stack_addr.get(0)) {
                        context.transfer_coins(Some(*addr), None, fee, false)?;
                    }

                    // transfer coins
                    if let (Some(coins), Some(from), Some(to)) =
                        (req.coins, call_stack_addr.get(0), call_stack_addr.get(1))
                    {
                        context.transfer_coins(Some(*from), Some(*to), coins, false)?;
                    }
                }

                // load and execute the compiled module
                // IMPORTANT: do not keep a lock here as `run_function` uses the `get_module` interface
                let module = self
                    .module_cache
                    .write()
                    .load_module(&bytecode, req.max_gas)?;

                let response = massa_sc_runtime::run_function(
                    &*self.execution_interface,
                    module,
                    &target_func,
                    &parameter,
                    req.max_gas,
                    self.config.gas_costs.clone(),
                );

                match response {
                    Ok(Response { init_gas_cost, .. })
                    | Err(VMError::ExecutionError { init_gas_cost, .. }) => {
                        self.module_cache
                            .write()
                            .set_init_cost(&bytecode, init_gas_cost);
                    }
                    _ => (),
                }

                response.map_err(|error| ExecutionError::VMError {
                    context: "ReadOnlyExecutionTarget::FunctionCall".to_string(),
                    error,
                })?
            }
        };

        // return the execution output
        let execution_output = context_guard!(self).settle_slot(None);
        let exact_exec_cost = req.max_gas.saturating_sub(exec_response.remaining_gas);

        // compute a gas cost, estimating the gas of the last SC call to be max_instance_cost
        let corrected_cost = match (context_guard!(self)).gas_remaining_before_subexecution {
            Some(gas_remaining) => req
                .max_gas
                .saturating_sub(gas_remaining) // yield gas used until last subexecution
                .saturating_add(self.config.gas_costs.max_instance_cost),
            None => self.config.gas_costs.max_instance_cost, // no subexecution, just max_instance_cost
        };

        // keep the max of the two so the last SC call has at least max_instance_cost of gas
        let estimated_cost = u64::max(exact_exec_cost, corrected_cost);
        debug!(
            "execute_readonly_request:
            exec_response.remaining_gas: {}
            exact_exec_cost: {}
            corrected_cost: {}
            estimated_cost: {}",
            exec_response.remaining_gas, exact_exec_cost, corrected_cost, estimated_cost
        );

        Ok(ReadOnlyExecutionOutput {
            out: execution_output,
            gas_cost: estimated_cost,
            call_result: exec_response.ret,
        })
    }

    /// Gets a balance both at the latest final and candidate executed slots
    pub fn get_final_and_candidate_balance(
        &self,
        address: &Address,
    ) -> (Option<Amount>, Option<Amount>) {
        let final_balance = self.final_state.read().get_ledger().get_balance(address);
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

    /// Gets a balance both at the latest final and candidate executed slots
    pub fn get_final_and_active_bytecode(
        &self,
        address: &Address,
    ) -> (Option<Bytecode>, Option<Bytecode>) {
        let final_bytecode = self.final_state.read().get_ledger().get_bytecode(address);
        let search_result = self.active_history.read().fetch_bytecode(address);
        let speculative_v = match search_result {
            HistorySearchResult::Present(active_bytecode) => Some(active_bytecode),
            HistorySearchResult::NoInfo => final_bytecode.clone(),
            HistorySearchResult::Absent => None,
        };
        (final_bytecode, speculative_v)
    }

    /// Gets roll counts both at the latest final and active executed slots
    pub fn get_final_and_candidate_rolls(&self, address: &Address) -> (u64, u64) {
        let final_rolls = self
            .final_state
            .read()
            .get_pos_state()
            .get_rolls_for(address);
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
        let final_entry = self
            .final_state
            .read()
            .get_ledger()
            .get_data_entry(address, key);
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
    #[allow(clippy::type_complexity)]
    pub fn get_final_and_candidate_datastore_keys(
        &self,
        addr: &Address,
        prefix: &[u8],
    ) -> (Option<BTreeSet<Vec<u8>>>, Option<BTreeSet<Vec<u8>>>) {
        // here, get the final keys from the final ledger, and make a copy of it for the candidate list
        // let final_keys = final_state.read().ledger.get_datastore_keys(addr);
        let final_keys = self
            .final_state
            .read()
            .get_ledger()
            .get_datastore_keys(addr, prefix);

        let mut candidate_keys = final_keys.clone();

        // compute prefix range
        let prefix_range = get_prefix_bounds(prefix);
        let range_ref = (prefix_range.0.as_ref(), prefix_range.1.as_ref());

        // traverse the history from oldest to newest, applying additions and deletions
        for output in &self.active_history.read().0 {
            match output.state_changes.ledger_changes.get(addr) {
                // address absent from the changes
                None => (),

                // address ledger entry being reset to an absolute new list of keys
                Some(SetUpdateOrDelete::Set(new_ledger_entry)) => {
                    candidate_keys = Some(
                        new_ledger_entry
                            .datastore
                            .range::<Vec<u8>, _>(range_ref)
                            .map(|(k, _v)| k.clone())
                            .collect(),
                    );
                }

                // address ledger entry being updated
                Some(SetUpdateOrDelete::Update(entry_updates)) => {
                    let c_k = candidate_keys.get_or_insert_with(Default::default);
                    for (ds_key, ds_update) in
                        entry_updates.datastore.range::<Vec<u8>, _>(range_ref)
                    {
                        match ds_update {
                            SetOrDelete::Set(_) => c_k.insert(ds_key.clone()),
                            SetOrDelete::Delete => c_k.remove(ds_key),
                        };
                    }
                }

                // address ledger entry being deleted
                Some(SetUpdateOrDelete::Delete) => {
                    candidate_keys = None;
                }
            }
        }

        (final_keys, candidate_keys)
    }

    pub fn get_address_cycle_infos(&self, address: &Address) -> Vec<ExecutionAddressCycleInfo> {
        context_guard!(self).get_address_cycle_infos(address, self.config.periods_per_cycle)
    }

    /// Returns for a given cycle the stakers taken into account
    /// by the selector. That correspond to the `roll_counts` in `cycle - 3`.
    ///
    /// By default it returns an empty map.
    pub fn get_cycle_active_rolls(&self, cycle: u64) -> BTreeMap<Address, u64> {
        self.final_state
            .read()
            .get_pos_state()
            .get_all_active_rolls(cycle)
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

    /// Check if a denunciation has been executed given a `DenunciationIndex`
    /// Returns a tuple of booleans:
    /// * first boolean is true if the denunciation has been executed speculatively
    /// * second boolean is true if the denunciation has been executed in the final state
    pub fn get_denunciation_execution_status(
        &self,
        denunciation_index: &DenunciationIndex,
    ) -> (bool, bool) {
        // check final state
        let executed_final = self
            .final_state
            .read()
            .get_executed_denunciations()
            .contains(denunciation_index);
        if executed_final {
            return (true, true);
        }

        // check active history
        let executed_candidate = {
            matches!(
                self.active_history
                    .read()
                    .fetch_executed_denunciation(denunciation_index),
                HistorySearchResult::Present(())
            )
        };

        (executed_candidate, false)
    }

    /// Get cycle infos
    pub fn get_cycle_infos(
        &self,
        cycle: u64,
        restrict_to_addresses: Option<&PreHashSet<Address>>,
    ) -> Option<ExecutionQueryCycleInfos> {
        let final_state_lock = self.final_state.read();

        // check if cycle is complete
        let is_final = match final_state_lock.get_pos_state().is_cycle_complete(cycle) {
            Some(v) => v,
            None => return None,
        };

        // active rolls
        let staker_infos: BTreeMap<Address, ExecutionQueryStakerInfo>;
        if let Some(addrs) = restrict_to_addresses {
            staker_infos = addrs
                .iter()
                .map(|addr| {
                    let staker_info = ExecutionQueryStakerInfo {
                        active_rolls: final_state_lock
                            .get_pos_state()
                            .get_address_active_rolls(addr, cycle)
                            .unwrap_or(0),
                        production_stats: final_state_lock
                            .get_pos_state()
                            .get_production_stats_for_address(cycle, addr)
                            .unwrap_or_default(),
                    };
                    (*addr, staker_info)
                })
                .collect()
        } else {
            let active_rolls = final_state_lock.get_pos_state().get_all_roll_counts(cycle);
            let production_stats = final_state_lock
                .get_pos_state()
                .get_all_production_stats(cycle)
                .unwrap_or_default();
            let all_addrs: BTreeSet<Address> = active_rolls
                .keys()
                .chain(production_stats.keys())
                .copied()
                .collect();
            staker_infos = all_addrs
                .into_iter()
                .map(|addr| {
                    let staker_info = ExecutionQueryStakerInfo {
                        active_rolls: active_rolls.get(&addr).copied().unwrap_or(0),
                        production_stats: production_stats.get(&addr).copied().unwrap_or_default(),
                    };
                    (addr, staker_info)
                })
                .collect()
        }

        // build result
        Some(ExecutionQueryCycleInfos {
            cycle,
            is_final,
            staker_infos,
        })
    }

    /// Get future deferred credits of an address
    pub fn get_address_future_deferred_credits(
        &self,
        address: &Address,
        max_slot: std::ops::Bound<Slot>,
    ) -> BTreeMap<Slot, Amount> {
        context_guard!(self).get_address_future_deferred_credits(
            address,
            self.config.thread_count,
            max_slot,
        )
    }

    /// Get future deferred credits of an address
    /// Returns tuple: (speculative, final)
    pub fn get_address_deferred_credits(
        &self,
        address: &Address,
    ) -> (BTreeMap<Slot, Amount>, BTreeMap<Slot, Amount>) {
        // get values from final state
        let res_final: BTreeMap<Slot, Amount> = self
            .final_state
            .read()
            .get_pos_state()
            .get_deferred_credits_range(.., Some(address))
            .credits
            .iter()
            .filter_map(|(slot, addr_amount)| {
                addr_amount.get(address).map(|amount| (*slot, *amount))
            })
            .collect();

        // get values from active history, backwards
        let mut res_speculative: BTreeMap<Slot, Amount> = BTreeMap::default();
        for hist_item in self.active_history.read().0.iter().rev() {
            for (slot, addr_amount) in &hist_item.state_changes.pos_changes.deferred_credits.credits
            {
                if let Some(amount) = addr_amount.get(address) {
                    res_speculative.entry(*slot).or_insert(*amount);
                };
            }
        }

        // fill missing speculative entries with final entries
        for (slot, amount) in &res_final {
            res_speculative.entry(*slot).or_insert(*amount);
        }

        // remove zero entries from speculative
        res_speculative.retain(|_s, a| !a.is_zero());

        (res_speculative, res_final)
    }

    /// Get the execution status of a batch of operations.
    ///
    ///  Return value: vector of
    ///  `(Option<speculative_status>, Option<final_status>)`
    ///  If an Option is None it means that the op execution was not found.
    ///  Note that old op executions are forgotten.
    /// Otherwise, the status is a boolean indicating whether the execution was successful (true) or if there was an error (false.)
    pub fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<(Option<bool>, Option<bool>)> {
        let speculative_exec = self.active_history.read().get_ops_exec_status(batch);
        let final_exec = self.final_state.read().get_ops_exec_status(batch);
        speculative_exec
            .into_iter()
            .zip(final_exec)
            .map(|(speculative_v, final_v)| {
                match (speculative_v, final_v) {
                    (None, Some(f)) => (Some(f), Some(f)), // special case: a final execution should also appear as speculative
                    (s, f) => (s, f),
                }
            })
            .collect()
    }

    /// Update MipStore with block header stats
    pub fn update_versioning_stats(&mut self, block_info: &Option<ExecutedBlockInfo>, slot: &Slot) {
        let slot_ts = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            *slot,
        )
        .expect("Cannot get timestamp from slot");

        self.mip_store.update_network_version_stats(
            slot_ts,
            block_info
                .as_ref()
                .map(|i| (i.current_version, i.announced_version)),
        );
    }
}
