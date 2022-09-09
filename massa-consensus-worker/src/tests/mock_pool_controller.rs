// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block::BlockId, endorsement::EndorsementId, operation::OperationId, slot::Slot,
};
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
    fn add_endorsements(&mut self, _endorsements: Storage) {
        todo!("")
    }
    fn add_operations(&mut self, _operations: Storage) {
        todo!("")
    }
    fn notify_final_cs_periods(&mut self, _final_cs_periods: &[u64]) {
        todo!("")
    }
    fn get_block_operations(&self, _slot: &Slot) -> (Vec<OperationId>, Storage) {
        todo!("")
    }
    fn get_block_endorsements(
        &self,
        _target_block: &BlockId,
        _target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        todo!("")
    }

    fn contains_endorsements(&self, _endorsements: &[EndorsementId]) -> Vec<bool> {
        todo!("")
    }

    fn contains_operations(&self, _operations: &[OperationId]) -> Vec<bool> {
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
