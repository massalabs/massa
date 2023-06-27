// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting with the Execution worker

use crate::types::{ExecutionQueryRequest, ExecutionQueryResponse, ReadOnlyExecutionRequest};
use crate::ExecutionError;
use crate::{ExecutionAddressInfo, ReadOnlyExecutionOutput};
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::block_id::BlockId;
use massa_models::denunciation::DenunciationIndex;
use massa_models::execution::EventFilter;
use massa_models::operation::OperationId;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::PreHashMap;
use massa_models::slot::Slot;
use massa_models::stats::ExecutionStats;
use massa_storage::Storage;
use std::collections::BTreeMap;
use std::collections::HashMap;

#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
/// interface that communicates with the execution worker thread
pub trait ExecutionController: Send + Sync {
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
    );

    /// Atomically query the execution state with multiple requests
    fn query_state(&self, req: ExecutionQueryRequest) -> ExecutionQueryResponse;

    /// Get execution events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent>;

    /// Get the final and active values of balance.
    ///
    /// # Return value
    /// * `(final_balance, active_balance)`
    fn get_final_and_candidate_balance(
        &self,
        addresses: &[Address],
    ) -> Vec<(Option<Amount>, Option<Amount>)>;

    /// Get the execution status of a batch of operations.
    ///
    ///  Return value: vector of
    ///  `(Option<speculative_status>, Option<final_status>)`
    ///  If an Option is None it means that the op execution was not found.
    ///  Note that old op executions are forgotten.
    /// Otherwise, the status is a boolean indicating whether the execution was successful (true) or if there was an error (false.)
    fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<(Option<bool>, Option<bool>)>;

    /// Get a copy of a single datastore entry with its final and active values
    ///
    /// # Return value
    /// * `(final_data_entry, active_data_entry)`
    #[allow(clippy::type_complexity)]
    fn get_final_and_active_data_entry(
        &self,
        input: Vec<(Address, Vec<u8>)>,
    ) -> Vec<(Option<Vec<u8>>, Option<Vec<u8>>)>;

    /// Returns for a given cycle the stakers taken into account
    /// by the selector. That correspond to the `roll_counts` in `cycle - 3`.
    ///
    /// By default it returns an empty map.
    fn get_cycle_active_rolls(&self, cycle: u64) -> BTreeMap<Address, u64>;

    /// Execute read-only SC function call without causing modifications to the consensus state
    ///
    /// # arguments
    /// * `req`: an instance of `ReadOnlyCallRequest` describing the parameters of the execution
    ///
    /// # returns
    /// An instance of `ExecutionOutput` containing a summary of the effects of the execution,
    /// or an error if the execution failed.
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ReadOnlyExecutionOutput, ExecutionError>;

    /// Check if a denunciation has been executed given a `DenunciationIndex`
    fn get_denunciation_execution_status(
        &self,
        denunciation_index: &DenunciationIndex,
    ) -> (bool, bool);

    /// Gets information about a batch of addresses
    fn get_addresses_infos(&self, addresses: &[Address]) -> Vec<ExecutionAddressInfo>;

    /// Get execution statistics
    fn get_stats(&self) -> ExecutionStats;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn ExecutionController>`.
    fn clone_box(&self) -> Box<dyn ExecutionController>;
}

/// Allow cloning `Box<dyn ExecutionController>`
/// Uses `ExecutionController::clone_box` internally
impl Clone for Box<dyn ExecutionController> {
    fn clone(&self) -> Box<dyn ExecutionController> {
        self.clone_box()
    }
}

/// Execution manager used to stop the execution thread
pub trait ExecutionManager {
    /// Stop the execution thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of `Box<dyn ExecutionManager>`
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
