// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements an execution controller.
//! See `massa-execution-exports/controller_traits.rs` for functional details.

use crate::execution::ExecutionState;
use crate::request_queue::{RequestQueue, RequestWithResponseSender};
use massa_channel::MassaChannel;
use massa_execution_exports::{
    ExecutionAddressInfo, ExecutionConfig, ExecutionController, ExecutionError, ExecutionManager,
    ExecutionQueryError, ExecutionQueryExecutionStatus, ExecutionQueryRequest,
    ExecutionQueryRequestItem, ExecutionQueryResponse, ExecutionQueryResponseItem,
    ReadOnlyExecutionOutput, ReadOnlyExecutionRequest,
};
use massa_models::denunciation::DenunciationIndex;
use massa_models::execution::EventFilter;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashMap;
use massa_models::stats::ExecutionStats;
use massa_models::{address::Address, amount::Amount, operation::OperationId};
use massa_models::{block_id::BlockId, slot::Slot};
use massa_storage::Storage;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Display;
use std::sync::Arc;
use tracing::info;

/// structure used to communicate with execution thread
pub(crate) struct ExecutionInputData {
    /// set stop to true to stop the thread
    pub stop: bool,
    /// list of newly finalized blocks
    pub finalized_blocks: HashMap<Slot, BlockId>,
    /// new blockclique (if there is a new one)
    pub new_blockclique: Option<HashMap<Slot, BlockId>>,
    /// storage instances for previously unprocessed blocks
    pub block_storage: PreHashMap<BlockId, Storage>,
    /// queue for read-only execution requests and response MPSCs to send back their outputs
    pub readonly_requests: RequestQueue<ReadOnlyExecutionRequest, ReadOnlyExecutionOutput>,
}

impl Display for ExecutionInputData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "stop={:?}, finalized={:?}, blockclique={:?}, readonly={:?}",
            self.stop,
            self.finalized_blocks
                .iter()
                .map(|(slot, id)| (*slot, *id))
                .collect::<BTreeMap<Slot, BlockId>>(),
            self.new_blockclique.as_ref().map(|bq| bq
                .iter()
                .map(|(slot, id)| (*slot, *id))
                .collect::<BTreeMap<Slot, BlockId>>()),
            self.readonly_requests
        )
    }
}

impl ExecutionInputData {
    /// Creates a new empty `ExecutionInputData`
    pub fn new(config: ExecutionConfig) -> Self {
        ExecutionInputData {
            stop: Default::default(),
            finalized_blocks: Default::default(),
            new_blockclique: Default::default(),
            block_storage: Default::default(),
            readonly_requests: RequestQueue::new(config.max_final_events),
        }
    }

    /// Takes the current input data into a clone that is returned,
    /// and resets self.
    pub fn take(&mut self) -> Self {
        let max_final_events = self.readonly_requests.capacity();
        ExecutionInputData {
            stop: std::mem::take(&mut self.stop),
            finalized_blocks: std::mem::take(&mut self.finalized_blocks),
            new_blockclique: std::mem::take(&mut self.new_blockclique),
            block_storage: std::mem::take(&mut self.block_storage),
            readonly_requests: std::mem::replace(
                &mut self.readonly_requests,
                RequestQueue::new(max_final_events),
            ),
        }
    }
}

#[derive(Clone)]
/// implementation of the execution controller
pub struct ExecutionControllerImpl {
    /// input data to process in the VM loop
    /// with a wake-up condition variable that needs to be triggered when the data changes
    pub(crate) input_data: Arc<(Condvar, Mutex<ExecutionInputData>)>,
    /// current execution state (see execution.rs for details)
    pub(crate) execution_state: Arc<RwLock<ExecutionState>>,
}

impl ExecutionController for ExecutionControllerImpl {
    /// Updates blockclique status by signaling newly finalized blocks and the latest blockclique.
    ///
    /// # Arguments
    /// * `finalized_blocks`: newly finalized blocks indexed by slot.
    /// * `blockclique`: new blockclique (if changed). Indexed by slot.
    /// * `block_storage`: storage instances for new blocks. Each one owns refs to the block and its ops/endorsements/parents.
    fn update_blockclique_status(
        &self,
        finalized_blocks: HashMap<Slot, BlockId>,
        new_blockclique: Option<HashMap<Slot, BlockId>>,
        block_storage: PreHashMap<BlockId, Storage>,
    ) {
        // lock input data
        let mut input_data = self.input_data.1.lock();

        // extend block info
        input_data.block_storage.extend(block_storage);

        // extend finalized blocks
        input_data.finalized_blocks.extend(finalized_blocks);

        // update blockclique
        if new_blockclique.is_some() {
            input_data.new_blockclique = new_blockclique;
        }

        // wake up VM loop
        self.input_data.0.notify_one();
    }

    /// Atomically query the execution state with multiple requests
    fn query_state(&self, req: ExecutionQueryRequest) -> ExecutionQueryResponse {
        let execution_lock = self.execution_state.read();
        let mut resp: ExecutionQueryResponse = ExecutionQueryResponse {
            responses: Vec::with_capacity(req.requests.len()),
            candidate_cursor: execution_lock.active_cursor,
            final_cursor: execution_lock.final_cursor,
            final_state_fingerprint: execution_lock.get_final_state_fingerprint(),
        };
        for req_item in req.requests {
            let resp_item = match req_item {
                ExecutionQueryRequestItem::AddressExistsCandidate(addr) => {
                    Ok(ExecutionQueryResponseItem::Boolean(
                        execution_lock
                            .get_final_and_candidate_balance(&addr)
                            .1
                            .is_some(),
                    ))
                }
                ExecutionQueryRequestItem::AddressExistsFinal(addr) => {
                    Ok(ExecutionQueryResponseItem::Boolean(
                        execution_lock
                            .get_final_and_candidate_balance(&addr)
                            .0
                            .is_some(),
                    ))
                }
                ExecutionQueryRequestItem::AddressBalanceCandidate(addr) => {
                    let (_final_v, speculative_v) =
                        execution_lock.get_final_and_candidate_balance(&addr);
                    match speculative_v {
                        Some(balance) => Ok(ExecutionQueryResponseItem::Amount(balance)),
                        None => Err(ExecutionQueryError::NotFound(format!("Account {}", addr))),
                    }
                }
                ExecutionQueryRequestItem::AddressBalanceFinal(addr) => {
                    let (final_v, _speculative_v) =
                        execution_lock.get_final_and_candidate_balance(&addr);
                    match final_v {
                        Some(balance) => Ok(ExecutionQueryResponseItem::Amount(balance)),
                        None => Err(ExecutionQueryError::NotFound(format!("Account {}", addr))),
                    }
                }
                ExecutionQueryRequestItem::AddressBytecodeCandidate(addr) => {
                    let (_final_v, speculative_v) =
                        execution_lock.get_final_and_active_bytecode(&addr);
                    match speculative_v {
                        Some(bytecode) => Ok(ExecutionQueryResponseItem::Bytecode(bytecode)),
                        None => Err(ExecutionQueryError::NotFound(format!("Account {}", addr))),
                    }
                }
                ExecutionQueryRequestItem::AddressBytecodeFinal(addr) => {
                    let (final_v, _speculative_v) =
                        execution_lock.get_final_and_active_bytecode(&addr);
                    match final_v {
                        Some(bytecode) => Ok(ExecutionQueryResponseItem::Bytecode(bytecode)),
                        None => Err(ExecutionQueryError::NotFound(format!("Account {}", addr))),
                    }
                }
                ExecutionQueryRequestItem::AddressDatastoreKeysCandidate { addr, prefix } => {
                    let (_final_v, speculative_v) =
                        execution_lock.get_final_and_candidate_datastore_keys(&addr, &prefix);
                    match speculative_v {
                        Some(keys) => Ok(ExecutionQueryResponseItem::KeyList(keys)),
                        None => Err(ExecutionQueryError::NotFound(format!("Account {}", addr))),
                    }
                }
                ExecutionQueryRequestItem::AddressDatastoreKeysFinal { addr, prefix } => {
                    let (final_v, _speculative_v) =
                        execution_lock.get_final_and_candidate_datastore_keys(&addr, &prefix);
                    match final_v {
                        Some(keys) => Ok(ExecutionQueryResponseItem::KeyList(keys)),
                        None => Err(ExecutionQueryError::NotFound(format!("Account {}", addr))),
                    }
                }
                ExecutionQueryRequestItem::AddressDatastoreValueCandidate { addr, key } => {
                    let (_final_v, speculative_v) =
                        execution_lock.get_final_and_active_data_entry(&addr, &key);
                    match speculative_v {
                        Some(value) => Ok(ExecutionQueryResponseItem::DatastoreValue(value)),
                        None => Err(ExecutionQueryError::NotFound(format!(
                            "Account {} datastore entry {:?}",
                            addr, key
                        ))),
                    }
                }
                ExecutionQueryRequestItem::AddressDatastoreValueFinal { addr, key } => {
                    let (final_v, _speculative_v) =
                        execution_lock.get_final_and_active_data_entry(&addr, &key);
                    match final_v {
                        Some(value) => Ok(ExecutionQueryResponseItem::DatastoreValue(value)),
                        None => Err(ExecutionQueryError::NotFound(format!(
                            "Account {} datastore entry {:?}",
                            addr, key
                        ))),
                    }
                }
                ExecutionQueryRequestItem::OpExecutionStatusCandidate(id) => {
                    let (speculative_v, _final_v) = execution_lock
                        .get_ops_exec_status(&[id])
                        .get(0)
                        .map(|(s_v, f_v)| (*s_v, *f_v))
                        .expect("expected one return value");
                    match speculative_v {
                        Some(true) => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::AlreadyExecutedWithSuccess,
                        )),
                        Some(false) => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::AlreadyExecutedWithFailure,
                        )),
                        None => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::ExecutableOrExpired,
                        )),
                    }
                }
                ExecutionQueryRequestItem::OpExecutionStatusFinal(id) => {
                    let (_speculative_v, final_v) = execution_lock
                        .get_ops_exec_status(&[id])
                        .get(0)
                        .map(|(s_v, f_v)| (*s_v, *f_v))
                        .expect("expected one return value");
                    match final_v {
                        Some(true) => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::AlreadyExecutedWithSuccess,
                        )),
                        Some(false) => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::AlreadyExecutedWithFailure,
                        )),
                        None => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::ExecutableOrExpired,
                        )),
                    }
                }
                ExecutionQueryRequestItem::DenunciationExecutionStatusCandidate(id) => {
                    let (speculative_v, _final_v) =
                        execution_lock.get_denunciation_execution_status(&id);
                    match speculative_v {
                        true => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::AlreadyExecutedWithSuccess,
                        )),
                        false => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::ExecutableOrExpired,
                        )),
                    }
                }
                ExecutionQueryRequestItem::DenunciationExecutionStatusFinal(id) => {
                    let (_speculative_v, final_v) =
                        execution_lock.get_denunciation_execution_status(&id);
                    match final_v {
                        true => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::AlreadyExecutedWithSuccess,
                        )),
                        false => Ok(ExecutionQueryResponseItem::ExecutionStatus(
                            ExecutionQueryExecutionStatus::ExecutableOrExpired,
                        )),
                    }
                }
                ExecutionQueryRequestItem::AddressRollsCandidate(addr) => {
                    let (_final_rolls, candidate_rolls) =
                        execution_lock.get_final_and_candidate_rolls(&addr);
                    Ok(ExecutionQueryResponseItem::RollCount(candidate_rolls))
                }
                ExecutionQueryRequestItem::AddressRollsFinal(addr) => {
                    let (final_rolls, _candidate_rolls) =
                        execution_lock.get_final_and_candidate_rolls(&addr);
                    Ok(ExecutionQueryResponseItem::RollCount(final_rolls))
                }
                ExecutionQueryRequestItem::AddressDeferredCreditsCandidate(addr) => {
                    let (candidate_v, _final_v) =
                        execution_lock.get_address_deferred_credits(&addr);
                    Ok(ExecutionQueryResponseItem::DeferredCredits(candidate_v))
                }
                ExecutionQueryRequestItem::AddressDeferredCreditsFinal(addr) => {
                    let (_candidate_v, final_v) =
                        execution_lock.get_address_deferred_credits(&addr);
                    Ok(ExecutionQueryResponseItem::DeferredCredits(final_v))
                }
                ExecutionQueryRequestItem::CycleInfos {
                    cycle,
                    restrict_to_addresses,
                } => {
                    let cycle_infos =
                        execution_lock.get_cycle_infos(cycle, restrict_to_addresses.as_ref());
                    match cycle_infos {
                        Some(cycle_infos) => {
                            Ok(ExecutionQueryResponseItem::CycleInfos(cycle_infos))
                        }
                        None => Err(ExecutionQueryError::NotFound(format!("Cycle {}", cycle))),
                    }
                }
                ExecutionQueryRequestItem::Events(filter) => {
                    Ok(ExecutionQueryResponseItem::Events(
                        execution_lock.get_filtered_sc_output_event(filter),
                    ))
                }
            };
            resp.responses.push(resp_item);
        }
        resp
    }

    /// Get the generated execution events, optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent> {
        self.execution_state
            .read()
            .get_filtered_sc_output_event(filter)
    }

    /// Get the final and candidate values of balance.
    ///
    /// # Return value
    /// * `(final_balance, candidate_balance)`
    fn get_final_and_candidate_balance(
        &self,
        addresses: &[Address],
    ) -> Vec<(Option<Amount>, Option<Amount>)> {
        let execution_state_lock = self.execution_state.read();
        let mut result = Vec::with_capacity(addresses.len());
        for addr in addresses {
            result.push(execution_state_lock.get_final_and_candidate_balance(addr));
        }
        result
    }

    /// Get a copy of a single datastore entry with its final and active values
    ///
    /// # Return value
    /// * `Vec<(final_data_entry, active_data_entry)>`
    fn get_final_and_active_data_entry(
        &self,
        input: Vec<(Address, Vec<u8>)>,
    ) -> Vec<(Option<Vec<u8>>, Option<Vec<u8>>)> {
        let mut result = Vec::with_capacity(input.len());
        let execution_state_lock = self.execution_state.read();
        for (addr, key) in input {
            result.push(execution_state_lock.get_final_and_active_data_entry(&addr, &key));
        }
        result
    }

    /// Return the active rolls distribution for the given `cycle`
    fn get_cycle_active_rolls(&self, cycle: u64) -> BTreeMap<Address, u64> {
        self.execution_state.read().get_cycle_active_rolls(cycle)
    }

    /// Executes a read-only request
    /// Read-only requests do not modify consensus state
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ReadOnlyExecutionOutput, ExecutionError> {
        let resp_rx = {
            let mut input_data = self.input_data.1.lock();

            // if the read-only queue is already full, return an error
            if input_data.readonly_requests.is_full() {
                return Err(ExecutionError::ChannelError(
                    "too many queued readonly requests".into(),
                ));
            }

            // prepare the channel to send back the result of the read-only execution
            let (resp_tx, resp_rx) = MassaChannel::new("read_only_request".to_string(), None);

            // append the request to the queue of input read-only requests
            input_data
                .readonly_requests
                .push(RequestWithResponseSender::new(req, resp_tx));

            // wake up the execution main loop
            self.input_data.0.notify_one();

            resp_rx
        };

        // Wait for the result of the execution
        match resp_rx.recv() {
            Ok(result) => result,
            Err(err) => Err(ExecutionError::ChannelError(format!(
                "readonly execution response channel readout failed: {}",
                err
            ))),
        }
    }

    /// Check if a denunciation has been executed given a `DenunciationIndex`
    /// Returns a tuple of booleans: `(speculative_execution_status, final_execution_status)`
    fn get_denunciation_execution_status(
        &self,
        denunciation_index: &DenunciationIndex,
    ) -> (bool, bool) {
        self.execution_state
            .read()
            .get_denunciation_execution_status(denunciation_index)
    }

    /// Gets information about a batch of addresses
    fn get_addresses_infos(&self, addresses: &[Address]) -> Vec<ExecutionAddressInfo> {
        let mut res = Vec::with_capacity(addresses.len());
        let exec_state = self.execution_state.read();
        for addr in addresses {
            let (final_datastore_keys, candidate_datastore_keys) =
                exec_state.get_final_and_candidate_datastore_keys(addr, &[]);
            let (final_balance, candidate_balance) =
                exec_state.get_final_and_candidate_balance(addr);
            let (final_roll_count, candidate_roll_count) =
                exec_state.get_final_and_candidate_rolls(addr);
            res.push(ExecutionAddressInfo {
                final_datastore_keys: final_datastore_keys.unwrap_or_default(),
                candidate_datastore_keys: candidate_datastore_keys.unwrap_or_default(),
                final_balance: final_balance.unwrap_or_default(),
                candidate_balance: candidate_balance.unwrap_or_default(),
                final_roll_count,
                candidate_roll_count,
                future_deferred_credits: exec_state.get_address_future_deferred_credits(addr),
                cycle_infos: exec_state.get_address_cycle_infos(addr),
            });
        }
        res
    }

    /// Get execution statistics
    fn get_stats(&self) -> ExecutionStats {
        self.execution_state.read().get_stats()
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn ExecutionController>`,
    /// see `massa-execution-exports/controller_traits.rs`
    fn clone_box(&self) -> Box<dyn ExecutionController> {
        Box::new(self.clone())
    }

    /// See trait definition
    fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<(Option<bool>, Option<bool>)> {
        self.execution_state.read().get_ops_exec_status(batch)
    }
}

/// Execution manager
/// Allows stopping the execution worker
pub struct ExecutionManagerImpl {
    /// input data to process in the VM loop
    /// with a wake-up condition variable that needs to be triggered when the data changes
    pub(crate) input_data: Arc<(Condvar, Mutex<ExecutionInputData>)>,
    /// handle used to join the worker thread
    pub(crate) thread_handle: Option<std::thread::JoinHandle<()>>,
}

impl ExecutionManager for ExecutionManagerImpl {
    /// stops the worker
    fn stop(&mut self) {
        info!("stopping Execution controller...");
        // notify the worker thread to stop
        {
            let mut input_wlock = self.input_data.1.lock();
            input_wlock.stop = true;
            self.input_data.0.notify_one();
        }
        // join the execution thread
        if let Some(join_handle) = self.thread_handle.take() {
            join_handle.join().expect("VM controller thread panicked");
        }
        info!("execution controller stopped");
    }
}
