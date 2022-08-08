// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{BlockId, EndorsementId, OperationId, Slot};
use massa_storage::Storage;

/// Trait defining a pool controller
pub trait PoolController: Send + Sync {
    /// add operations to pool
    fn add_operations(&mut self, ops: Storage);

    /// add endorsements to pool
    fn add_endorsements(&mut self, endorsements: Storage);

    /// notify of new consensus final periods
    fn notify_final_cs_periods(&mut self, final_cs_periods: &Vec<u64>);

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage);

    /// get endorsements for a block
    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage);

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
