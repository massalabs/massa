use crate::controller_impl::PoolControllerImpl;
use crate::operation_pool::OperationPool;
use massa_pool_exports::PoolConfig;
use std::sync::{Arc, RwLock};

pub fn start_pool(config: PoolConfig) -> PoolControllerImpl {
    // start operation pool
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(config.clone())));

    // start endorsement pool
    //TODO

    PoolControllerImpl {
        config,
        operation_pool,
    }
}
