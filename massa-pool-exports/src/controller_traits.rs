// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::denunciation::Denunciation;
use massa_models::{
    block_id::BlockId, endorsement::EndorsementId, operation::OperationId, slot::Slot,
};
use massa_storage::Storage;

/// Trait defining a pool controller
pub trait PoolController: Send + Sync {
    /// Asynchronously add operations to pool. Simply print a warning on failure.
    fn add_operations(&mut self, ops: Storage);

    /// Asynchronously add endorsements to pool. Simply print a warning on failure.
    fn add_endorsements(&mut self, endorsements: Storage);

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

    /// Get the number of endorsements in the pool
    fn get_endorsement_count(&self) -> usize;

    /// Get the number of operations in the pool
    fn get_operation_count(&self) -> usize;

    /// Check if the pool contains a list of endorsements. Returns one boolean per item.
    fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool>;

    /// Check if the pool contains a list of operations. Returns one boolean per item.
    fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool>;

    /// Add denunciations to pool. Simply print a warning on failure.
    fn add_denunciation(&mut self, denunciation: Denunciation);

    /// Get the number of denunciations in the pool
    fn get_denunciation_count(&self) -> usize;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn PoolController>`.
    fn clone_box(&self) -> Box<dyn PoolController>;

    /// Get final cs periods (updated regularly from consensus)
    fn get_final_cs_periods(&self) -> &Vec<u64>;
}

/// Allow cloning `Box<dyn PoolController>`
/// Uses `PoolController::clone_box` internally
impl Clone for Box<dyn PoolController> {
    fn clone(&self) -> Box<dyn PoolController> {
        self.clone_box()
    }
}

/// Pool manager trait
pub trait PoolManager: Send + Sync {
    /// Stops the worker
    fn stop(&mut self);
}
