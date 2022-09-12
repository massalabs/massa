use massa_models::{
    block::BlockId, endorsement::EndorsementId, operation::OperationId, slot::Slot,
};
use massa_pool_exports::{PoolConfig, PoolController};
use massa_storage::Storage;
use parking_lot::RwLock;
use std::sync::Arc;

use crate::{endorsement_pool::EndorsementPool, operation_pool::OperationPool};
#[derive(Clone)]
pub struct PoolControllerImpl {
    pub(crate) _config: PoolConfig,
    pub(crate) operation_pool: Arc<RwLock<OperationPool>>,
    pub(crate) endorsement_pool: Arc<RwLock<EndorsementPool>>,
}

impl PoolController for PoolControllerImpl {
    /// add operations to pool
    fn add_operations(&mut self, ops: Storage) {
        self.operation_pool.write().add_operations(ops);
    }

    /// add endorsements to pool
    fn add_endorsements(&mut self, endorsements: Storage) {
        self.endorsement_pool.write().add_endorsements(endorsements);
    }

    /// notify of new final consensus periods (1 per thread)
    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        self.operation_pool
            .write()
            .notify_final_cs_periods(final_cs_periods);
        self.endorsement_pool
            .write()
            .notify_final_cs_periods(final_cs_periods);
    }

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        self.operation_pool.read().get_block_operations(slot)
    }

    /// get endorsements for a block
    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        self.endorsement_pool
            .read()
            .get_block_endorsements(target_slot, target_block)
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn PoolController>`,
    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }

    /// Get the number of endorsements in the pool
    fn get_endorsement_count(&self) -> usize {
        self.endorsement_pool.read().len()
    }

    /// Get the number of operations in the pool
    fn get_operation_count(&self) -> usize {
        self.operation_pool.read().len()
    }

    /// Check if the pool contains a list of endorsements. Returns one boolean per item.
    fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool> {
        let lck = self.endorsement_pool.read();
        endorsements.iter().map(|id| lck.contains(id)).collect()
    }

    /// Check if the pool contains a list of operations. Returns one boolean per item.
    fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool> {
        let lck = self.operation_pool.read();
        operations.iter().map(|id| lck.contains(id)).collect()
    }
}
