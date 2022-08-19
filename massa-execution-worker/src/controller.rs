// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module implements an execution controller.
//! See `massa-execution-exports/controller_traits.rs` for functional details.

use crate::execution::ExecutionState;
use crate::request_queue::{RequestQueue, RequestWithResponseSender};
use massa_execution_exports::{
    ExecutionConfig, ExecutionController, ExecutionError, ExecutionManager, ExecutionOutput,
    ReadOnlyExecutionRequest,
};
use massa_models::api::EventFilter;
use massa_models::execution::{ExecutionAddressInfo, ExecutionStatus};
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::{Map, Set};
use massa_models::{Address, Amount, OperationId};
use massa_models::{BlockId, Slot};
use massa_storage::Storage;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tracing::info;

/// structure used to communicate with execution thread
pub(crate) struct ExecutionInputData {
    /// set stop to true to stop the thread
    pub stop: bool,
    /// list of newly finalized blocks, indexed by slot
    pub finalized_blocks: HashMap<Slot, (BlockId, Storage)>,
    /// new blockclique (if there is a new one), blocks indexed by slot
    pub new_blockclique: Option<HashMap<Slot, (BlockId, Storage)>>,
    /// queue for read-only execution requests and response MPSCs to send back their outputs
    pub readonly_requests: RequestQueue<ReadOnlyExecutionRequest, ExecutionOutput>,
}

impl ExecutionInputData {
    /// Creates a new empty `ExecutionInputData`
    pub fn new(config: ExecutionConfig) -> Self {
        ExecutionInputData {
            stop: Default::default(),
            finalized_blocks: Default::default(),
            new_blockclique: Default::default(),
            readonly_requests: RequestQueue::new(config.max_final_events),
        }
    }

    /// Takes the current input data into a clone that is returned,
    /// and resets self.
    pub fn take(&mut self) -> Self {
        ExecutionInputData {
            stop: std::mem::take(&mut self.stop),
            finalized_blocks: std::mem::take(&mut self.finalized_blocks),
            new_blockclique: std::mem::take(&mut self.new_blockclique),
            readonly_requests: self.readonly_requests.take(),
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
    /// called to signal changes on the current blockclique, also listing newly finalized blocks
    ///
    /// # arguments
    /// * `finalized_blocks`: list of newly finalized blocks to be appended to the input finalized blocks. Each Storage owns the block and its ops/endorsements/parents.
    /// * `blockclique`: new blockclique, replaces the current one in the input. Each Storage owns the block and its ops/endorsements/parents.
    fn update_blockclique_status(
        &self,
        finalized_blocks: HashMap<Slot, (BlockId, Storage)>,
        new_blockclique: HashMap<Slot, (BlockId, Storage)>,
    ) {
        // update input data
        let mut input_data = self.input_data.1.lock();
        input_data.new_blockclique = Some(new_blockclique); // replace blockclique
        input_data.finalized_blocks.extend(finalized_blocks); // append finalized blocks
        self.input_data.0.notify_one(); // wake up VM loop
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

    /// Get a balance final and active values
    ///
    /// # Return value
    /// * `(final_balance, active_balance)`
    fn get_final_and_active_parallel_balance(
        &self,
        addresses: &[Address],
    ) -> Vec<(Option<Amount>, Option<Amount>)> {
        let lock = self.execution_state.read();
        let mut result = Vec::with_capacity(addresses.len());
        for addr in addresses {
            result.push(lock.get_final_and_active_parallel_balance(addr));
        }
        result
    }

    /// Get the final and active values of sequential balances.
    ///
    /// # Return value
    /// * `(final_balance, active_balance)`
    fn get_final_and_active_sequential_balance(
        &self,
        addresses: &[Address],
    ) -> Vec<(Option<Amount>, Option<Amount>)> {
        let lock = self.execution_state.read();
        let mut result = Vec::with_capacity(addresses.len());
        for addr in addresses {
            result.push(lock.get_final_and_active_sequential_balance(addr));
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
        let lock = self.execution_state.read();
        let mut result = Vec::with_capacity(input.len());
        for (addr, key) in input {
            result.push(lock.get_final_and_active_data_entry(&addr, &key));
        }
        result
    }

    /// Get every datastore key of the given address.
    ///
    /// # Returns
    /// A vector containing all the (final, active) keys
    fn get_final_and_active_datastore_keys(
        &self,
        addresses: &[Address],
    ) -> Vec<(BTreeSet<Vec<u8>>, BTreeSet<Vec<u8>>)> {
        let lock = self.execution_state.read();
        addresses
            .iter()
            .map(|addr| lock.get_final_and_active_datastore_keys(addr))
            .collect()
    }

    /// Get final and candidate information about addresses
    fn get_final_active_address_infos(
        &self,
        addresses: &[Address],
    ) -> Vec<(ExecutionAddressInfo, ExecutionAddressInfo)> {
        let lock = self.execution_state.read();
        addresses
            .iter()
            .map(|addr| {
                let (final_par_bal, active_par_bal) =
                    lock.get_final_and_active_parallel_balance(addr);
                let (final_seq_bal, active_seq_bal) =
                    lock.get_final_and_active_sequential_balance(addr);
                let (final_rolls, active_rolls) = lock.get_final_and_active_rolls(addr);
                let (final_keys, active_keys) = lock.get_final_and_active_datastore_keys(addr);
                let final_info = ExecutionAddressInfo {
                    parallel_balance: final_par_bal.unwrap_or_default(),
                    sequential_balance: final_seq_bal.unwrap_or_default(),
                    roll_count: final_rolls,
                    datastore_keys: final_keys,
                };
                let active_info = ExecutionAddressInfo {
                    parallel_balance: active_par_bal.unwrap_or_default(),
                    sequential_balance: active_seq_bal.unwrap_or_default(),
                    roll_count: active_rolls,
                    datastore_keys: active_keys,
                };
                (final_info, active_info)
            })
            .collect()
    }

    /// Gets the statuses of a list of operations
    fn get_operation_statuses(&self, ops: &[OperationId]) -> Vec<ExecutionStatus> {
        self.execution_state.read().get_operation_statuses(ops)
    }

    /// Return the final rolls distribution for the given `cycle`
    fn get_cycle_rolls(&self, cycle: u64) -> Map<Address, u64> {
        self.execution_state.read().get_cycle_rolls(cycle)
    }

    /// Executes a read-only request
    /// Read-only requests do not modify consensus state
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError> {
        let resp_rx = {
            let mut input_data = self.input_data.1.lock();

            // if the read-only queue is already full, return an error
            if input_data.readonly_requests.is_full() {
                return Err(ExecutionError::ChannelError(
                    "too many queued readonly requests".into(),
                ));
            }

            // prepare the channel to send back the result of the read-only execution
            let (resp_tx, resp_rx) =
                std::sync::mpsc::channel::<Result<ExecutionOutput, ExecutionError>>();

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

    /// List which operations inside the provided list were not executed
    fn unexecuted_ops_among(&self, ops: &Set<OperationId>, thread: u8) -> Set<OperationId> {
        self.execution_state
            .read()
            .unexecuted_ops_among(&ops, thread)
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn ExecutionController>`,
    /// see `massa-execution-exports/controller_traits.rs`
    fn clone_box(&self) -> Box<dyn ExecutionController> {
        Box::new(self.clone())
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
