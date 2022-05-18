// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting with the Execution worker

use crate::types::ExecutionOutput;
use crate::types::ReadOnlyExecutionRequest;
use crate::ExecutionError;
use massa_ledger as _;
use massa_models::api::EventFilter;
use massa_models::output_event::SCOutputEvent;
use massa_models::Address;
use massa_models::Amount;
use massa_models::BlockId;
use massa_models::Slot;
use std::collections::HashMap;

/// interface that communicates with the execution worker thread
pub trait ExecutionController: Send + Sync {
    /// Updates blockclique status by signaling newly finalized blocks and the latest blockclique.
    ///
    /// # Arguments
    /// * `finalized_blocks`: newly finalized blocks
    /// * `blockclique`: new blockclique
    fn update_blockclique_status(
        &self,
        finalized_blocks: HashMap<Slot, BlockId>,
        blockclique: HashMap<Slot, BlockId>,
    );

    /// Get execution events optionally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent>;

    /// TODO: documentation
    /// TODO: add handling for datastore
    fn get_final_and_active_parallel_balance(
        &self,
        address: &Address,
    ) -> (Option<Amount>, Option<Amount>);

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
    ) -> Result<ExecutionOutput, ExecutionError>;

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
    /// because it is not allowed to move out of Box<dyn ExecutionManager>
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
