// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{BlockId, OperationId, Slot, WrappedEndorsement};
use massa_storage::Storage;

/// Trait defining a pool controller
pub trait PoolController: Send + Sync {
    /// add operations to pool
    fn add_operations(&mut self, ops: Storage);

    /// add endorsements to pool
    fn add_endorsements(&mut self, endorsements: Vec<WrappedEndorsement>);

    /// notify of new final slot
    fn notify_final_slot(&mut self, slot: &Slot);

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage);

    /// get endorsements for a block
    fn get_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> Vec<Option<WrappedEndorsement>>;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn PoolController>`.
    fn clone_box(&self) -> Box<dyn PoolController>;
}
