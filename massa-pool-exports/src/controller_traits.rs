// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block::BlockId, endorsement::EndorsementId, operation::OperationId, slot::Slot,
};
use massa_storage::Storage;

/// Trait defining a pool controller
pub trait PoolController: Send + Sync {
    /// add operations to pool
    fn add_operations(&mut self, ops: Storage);

    /// add endorsements to pool
    fn add_endorsements(&mut self, endorsements: Storage);

    /// notify of new consensus final periods
    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]);

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage);

    /// get endorsements for a block
    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage);

    /// Get the number of endorsements in the pool
    fn get_endorsement_count(&self) -> usize;

    /// Get the number of operations in the pool
    fn get_operation_count(&self) -> usize;

    /// Check if the pool contains a list of endorsements. Returns one boolean per item.
    fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool>;

    /// Check if the pool contains a list of operations. Returns one boolean per item.
    fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool>;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn PoolController>`.
    fn clone_box(&self) -> Box<dyn PoolController>;
}

/// Allow cloning `Box<dyn PoolController>`
/// Uses `PoolController::clone_box` internally
impl Clone for Box<dyn PoolController> {
    fn clone(&self) -> Box<dyn PoolController> {
        self.clone_box()
    }
}
