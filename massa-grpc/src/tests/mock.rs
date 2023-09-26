use std::collections::{BTreeMap, HashMap};

use massa_execution_exports::{
    ExecutionAddressInfo, ExecutionBlockMetadata, ExecutionController, ExecutionError,
    ExecutionQueryRequest, ExecutionQueryResponse, ReadOnlyExecutionOutput,
    ReadOnlyExecutionRequest,
};
use massa_models::{
    address::Address,
    amount::Amount,
    block_id::BlockId,
    denunciation::{Denunciation, DenunciationIndex, DenunciationPrecursor},
    endorsement::EndorsementId,
    execution::EventFilter,
    operation::OperationId,
    output_event::SCOutputEvent,
    prehash::{PreHashMap, PreHashSet},
    slot::Slot,
    stats::ExecutionStats,
};
use massa_pool_exports::PoolController;
use massa_pos_exports::{PosResult, Selection, SelectorController};
use massa_storage::Storage;

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

#[cfg(any(test, feature = "testing"))]
mockall::mock! {
    pub PoolCtrl{}

    impl PoolController for PoolCtrl {
        /// Asynchronously add operations to pool. Simply print a warning on failure.
        fn add_operations(&mut self, ops: Storage);

        /// Asynchronously add endorsements to pool. Simply print a warning on failure.
        fn add_endorsements(&mut self, endorsements: Storage);

        /// Add denunciation precursor to pool
        fn add_denunciation_precursor(&self, denunciation_precursor: DenunciationPrecursor);

        /// Asynchronously notify of new consensus final periods. Simply print a warning on failure.
        fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]);

        /// Get operations for block creation.
        fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage);

        /// Get endorsements for a block.
        fn get_block_endorsements(
            &self,
            target_block: &BlockId,
            slot: &Slot,
        ) -> (Vec<Option<EndorsementId>>, Storage);

        /// Get denunciations for a block header.
        fn get_block_denunciations(&self, target_slot: &Slot) -> Vec<Denunciation>;

        /// Get the number of endorsements in the pool
        fn get_endorsement_count(&self) -> usize;

        /// Get the number of operations in the pool
        fn get_operation_count(&self) -> usize;

        /// Check if the pool contains a list of endorsements. Returns one boolean per item.
        fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool>;

        /// Check if the pool contains a list of operations. Returns one boolean per item.
        fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool>;

        /// Check if the pool contains a denunciation. Returns a boolean
        fn contains_denunciation(&self, denunciation: &Denunciation) -> bool;

        /// Get the number of denunciations in the pool
        fn get_denunciation_count(&self) -> usize;

        /// Returns a boxed clone of self.
        /// Useful to allow cloning `Box<dyn PoolController>`.
        fn clone_box(&self) -> Box<dyn PoolController>;

        /// Get final cs periods (updated regularly from consensus)
        fn get_final_cs_periods(&self) -> &Vec<u64>;
    }
}

#[cfg(any(test, feature = "testing"))]
mockall::mock! {
    pub SelectorCtrl {}

    impl SelectorController for SelectorCtrl {
        fn wait_for_draws(&self, cycle: u64) -> PosResult<u64>;

        fn feed_cycle(
            &self,
            cycle: u64,
            lookback_rolls: BTreeMap<Address, u64>,
            lookback_seed: massa_hash::Hash,
        ) -> PosResult<()>;

        fn get_selection(&self, slot: Slot) -> PosResult<Selection>;

        fn get_producer(&self, slot: Slot) -> PosResult<Address>;

        #[allow(clippy::needless_lifetimes)] // lifetime elision conflicts with Mockall
        fn get_available_selections_in_range<'a>(
            &self,
            slot_range: std::ops::RangeInclusive<Slot>,
            restrict_to_addresses: Option<&'a PreHashSet<Address>>,
        ) -> PosResult<BTreeMap<Slot, Selection>>;

        fn clone_box(&self) -> Box<dyn SelectorController>;

        #[cfg(feature = "testing")]
        fn get_entire_selection(&self) -> VecDeque<(u64, HashMap<Slot, Selection>)> {
            unimplemented!("mock implementation only")
        }
    }

}
