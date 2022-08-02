use std::sync::{Arc, RwLock};

use massa_models::{prehash::Set, BlockId, EndorsementId, OperationId, Slot, WrappedEndorsement};
use massa_pool_exports::{PoolConfig, PoolController, PoolOperationCursor};
use massa_storage::Storage;
#[derive(Clone)]
pub struct PoolControllerImpl {
    config: PoolConfig,
    operation_pool: Arc<RwLock<OperationPool>>,
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
    fn add_endorsements(&mut self, endorsements: Vec<WrappedEndorsement>) {
        //TODO
    }

    /// notify of new final slot
    fn notify_final_slot(&mut self, slot: &Slot) {
        self.operation_pool
            .write()
            .expect("could not w-lock operation pool")
            .notify_final_slot(slot);
    }

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        self.operation_pool
            .write()
            .expect("could not w-lock operation pool")
            .get_block_operations(slot)
    }

    /// get endorsements for a block
    fn get_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> Vec<Option<WrappedEndorsement>> {
        Default::default() //TODO
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn PoolController>`,
    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }
}
