use massa_models::{prehash::Set, BlockId, EndorsementId, OperationId, Slot, WrappedOperation};
use massa_pool_exports::{PoolConfig, PoolController};
use massa_storage::Storage;
use std::sync::{Arc, RwLock};

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
        self.operation_pool
            .write()
            .expect("could not w-lock operation pool")
            .add_operations(ops);
    }

    /// add endorsements to pool
    fn add_endorsements(&mut self, endorsements: Storage) {
        self.endorsement_pool
            .write()
            .expect("could not w-lock endorsement pool")
            .add_endorsements(endorsements);
    }

    /// notify of new final consensus periods (1 per thread)
    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        self.operation_pool
            .write()
            .expect("could not w-lock operation pool")
            .notify_final_cs_periods(final_cs_periods);
        self.endorsement_pool
            .write()
            .expect("could not w-lock endorsement pool")
            .notify_final_cs_periods(final_cs_periods);
    }

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        self.operation_pool
            .read()
            .expect("could not r-lock operation pool")
            .get_block_operations(slot)
    }

    /// get endorsements for a block
    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        self.endorsement_pool
            .read()
            .expect("could not r-lock endorsement pool")
            .get_block_endorsements(target_slot, target_block)
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn PoolController>`,
    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }

    /// Get respectivelly the number of operations and endorsements currently in pool
    fn get_stats(&self) -> (usize, usize) {
        (
            self.operation_pool
                .read()
                .expect("could not r-lock operation pool")
                .storage
                .local_operation_len(),
            self.endorsement_pool
                .read()
                .expect("could not r-lock endorsement pool")
                .storage
                .local_endorsement_len(),
        )
    }

    /// Get the operation pool storage
    fn get_operations_by_ids(&self, ids: &Set<OperationId>) -> Vec<WrappedOperation> {
        self.operation_pool
            .read()
            .expect("could not r-lock operation pool")
            .storage
            .get_operations_by_ids(ids)
    }

    /// Get the endorsement pool storage
    fn get_endorsement_ids(&self) -> Set<EndorsementId> {
        self.endorsement_pool
            .read()
            .expect("could not r-lock operation pool")
            .storage
            .get_local_endorsement_ids()
    }
}
