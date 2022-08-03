use massa_models::{BlockId, EndorsementId, OperationId, Slot};
use massa_pool_exports::{PoolConfig, PoolController};
use massa_storage::Storage;
use std::sync::{Arc, RwLock};

use crate::{endorsement_pool::EndorsementPool, operation_pool::OperationPool};
#[derive(Clone)]
pub struct PoolControllerImpl {
    pub(crate) config: PoolConfig,
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
    fn notify_final_cs_periods(&mut self, final_cs_periods: &Vec<u64>) {
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
}
