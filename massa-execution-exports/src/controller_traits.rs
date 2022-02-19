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

pub trait ExecutionController: Sync + Send {
    /// Update blockclique status
    ///
    /// # arguments
    /// * finalized_blocks: newly finalized blocks
    /// * blockclique: new blockclique
    fn update_blockclique_status(
        &self,
        finalized_blocks: Map<BlockId, Block>,
        blockclique: Map<BlockId, Block>,
    );

    /// Get events optionnally filtered by:
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

    /// gets a copy of a full ledger entry
    ///
    /// # return value
    /// * (final_entry, active_entry)
    fn get_full_ledger_entry(&self, addr: &Address) -> (Option<LedgerEntry>, Option<LedgerEntry>);

    /// Executes a readonly request
    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ExecutionOutput, ExecutionError>;
}

/// execution manager
pub trait ExecutionManager {
    /// stops the VM
    fn stop(&mut self);

    /// get a shared reference to the VM controller
    fn get_controller(&self) -> Box<dyn ExecutionController>;
}
