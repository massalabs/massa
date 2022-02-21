// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module exports generic traits representing interfaces for interacting with the Execution worker

use crate::types::ExecutionOutput;
use crate::types::ReadOnlyExecutionRequest;
use crate::ExecutionError;
use massa_ledger::LedgerEntry;
use massa_models::output_event::SCOutputEvent;
use massa_models::prehash::Map;
use massa_models::Address;
use massa_models::Block;
use massa_models::BlockId;
use massa_models::OperationId;
use massa_models::Slot;

/// interface that communicates with the execution worker thread
pub trait ExecutionController: Send + Sync {
    /// Updates blockclique status by signalling newly finalized blocks and the latest blockclique.
    ///
    /// # arguments
    /// * finalized_blocks: newly finalized blocks
    /// * blockclique: new blockclique
    fn update_blockclique_status(
        &self,
        finalized_blocks: Map<BlockId, Block>,
        blockclique: Map<BlockId, Block>,
    );

    /// Get execution events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    fn get_filtered_sc_output_event(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
    ) -> Vec<SCOutputEvent>;

    /// Get a copy of a full ledger entry
    ///
    /// # return value
    /// * (final_entry, active_entry)
    fn get_full_ledger_entry(&self, addr: &Address) -> (Option<LedgerEntry>, Option<LedgerEntry>);

    /// Execute read-only bytecode without causing modifications to the consensus state
    ///
    /// # arguments
    /// * req: an instance of ReadOnlyExecutionRequest describing the parameters of the execution
    ///
    /// # returns
    /// An instance of ExecutionOutput containing a summary of the effects of the execution,
    /// or an error if the execution failed.
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError>;
}

/// Execution manager used to generate controllers and to stop the execution thread
pub trait ExecutionManager {
    /// Stop the execution thread
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of Box<dyn ExecutionManager>
    fn stop(&mut self);

    /// Get a new execution controller
    fn get_controller(&self) -> Box<dyn ExecutionController>;
}
