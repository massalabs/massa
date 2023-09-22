use std::collections::{BTreeMap, HashMap};

use massa_execution_exports::{
    ExecutionAddressInfo, ExecutionBlockMetadata, ExecutionController, ExecutionError,
    ExecutionQueryRequest, ExecutionQueryResponse, ReadOnlyExecutionOutput,
    ReadOnlyExecutionRequest,
};
use massa_models::{
    address::Address, amount::Amount, block_id::BlockId, denunciation::DenunciationIndex,
    execution::EventFilter, operation::OperationId, output_event::SCOutputEvent,
    prehash::PreHashMap, slot::Slot, stats::ExecutionStats,
};

#[cfg(any(test, feature = "testing"))]
mockall::mock! {

    pub ExecutionCtrl {}
    impl Clone for ExecutionCtrl {
        fn clone(&self) -> Self;
    }

    impl ExecutionController for ExecutionCtrl {

    fn update_blockclique_status(
        &self,
        finalized_blocks: HashMap<Slot, BlockId>,
        new_blockclique: Option<HashMap<Slot, BlockId>>,
        block_metadata: PreHashMap<BlockId, ExecutionBlockMetadata>,
    );

    /// Atomically query the execution state with multiple requests
    fn query_state(&self, req: ExecutionQueryRequest) -> ExecutionQueryResponse;

    fn get_filtered_sc_output_event(&self, filter: EventFilter) -> Vec<SCOutputEvent>;

    fn get_final_and_candidate_balance(
        &self,
        addresses: &[Address],
    ) -> Vec<(Option<Amount>, Option<Amount>)>;

    fn get_ops_exec_status(&self, batch: &[OperationId]) -> Vec<(Option<bool>, Option<bool>)>;

    #[allow(clippy::type_complexity)]
    fn get_final_and_active_data_entry(
        &self,
        input: Vec<(Address, Vec<u8>)>,
    ) -> Vec<(Option<Vec<u8>>, Option<Vec<u8>>)>;

    fn get_cycle_active_rolls(&self, cycle: u64) -> BTreeMap<Address, u64>;

    fn execute_readonly_request(
        &self,
        req: ReadOnlyExecutionRequest,
    ) -> Result<ReadOnlyExecutionOutput, ExecutionError>;

    fn get_denunciation_execution_status(
        &self,
        denunciation_index: &DenunciationIndex,
    ) -> (bool, bool);

    fn get_addresses_infos(&self, addresses: &[Address]) -> Vec<ExecutionAddressInfo>;

    fn get_stats(&self) -> ExecutionStats;

    fn clone_box(&self) -> Box<dyn ExecutionController>;
    }
}
