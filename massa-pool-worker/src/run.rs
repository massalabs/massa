use crate::operation_pool::OperationPool;
use crate::{controller_impl::PoolControllerImpl, endorsement_pool::EndorsementPool};
use massa_execution_exports::ExecutionController;
use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use std::sync::{Arc, RwLock};

/// Starts the pool system and returns a controller
pub fn start_pool(
    config: PoolConfig,
    storage: &Storage,
    execution_controller: Box<dyn ExecutionController>,
) -> PoolControllerImpl {
    // start operation pool
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(
        config,
        storage,
        execution_controller,
    )));

    // start endorsement pool
    let endorsement_pool = Arc::new(RwLock::new(EndorsementPool::init(config, storage)));

    PoolControllerImpl {
        _config: config,
        operation_pool,
        endorsement_pool,
    }
}
