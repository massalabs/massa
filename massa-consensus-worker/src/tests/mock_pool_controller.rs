// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{prehash::Set, BlockId, EndorsementId, OperationId, Slot, WrappedOperation};
use massa_pool_exports::PoolController;
use massa_storage::Storage;

#[derive(Clone)]
pub struct MockPoolController;

impl MockPoolController {
    pub fn new() -> Self {
        MockPoolController
    }
}

impl PoolController for MockPoolController {
    fn add_endorsements(&mut self, endorsements: Storage) {
        todo!("")
    }
    fn add_operations(&mut self, operations: Storage) {
        todo!("")
    }
    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        todo!("")
    }
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        todo!("")
    }
    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        todo!("")
    }

    fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool> {
        todo!("")
    }

    fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool> {
        todo!("")
    }

    fn get_endorsement_count(&self) -> usize {
        todo!("")
    }

    fn get_operation_count(&self) -> usize {
        todo!("")
    }

    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }
}
