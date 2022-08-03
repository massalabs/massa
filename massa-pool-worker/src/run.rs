use crate::operation_pool::OperationPool;
use crate::{controller_impl::PoolControllerImpl, endorsement_pool::EndorsementPool};
use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use std::sync::{Arc, RwLock};

pub fn start_pool(config: PoolConfig, storage: Storage) -> PoolControllerImpl {
    // start operation pool
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(
        config.clone(),
        storage.clone_with_refs(),
    )));

    // start endorsement pool
    let endorsement_pool = Arc::new(RwLock::new(EndorsementPool::init(config.clone(), storage)));

    PoolControllerImpl {
        config,
        operation_pool,
        endorsement_pool,
    }
}
