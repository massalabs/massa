use crate::operation_pool::OperationPool;
use crate::protection::{
    start_protection_thread, PoolAddrInfo, ProtectionConfig, ProtectionControllerImpl,
};
use crate::{controller_impl::PoolControllerImpl, endorsement_pool::EndorsementPool};
use massa_execution_exports::ExecutionController;
use massa_models::address::Address;
use massa_models::prehash::PreHashMap;
use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use parking_lot::RwLock;
use std::sync::{mpsc, Arc};

/// Starts the pool system and returns a controller
#[cfg(test)]
pub fn start_pool_without_protection(
    config: PoolConfig,
    storage: &Storage,
    execution_controller: Box<dyn ExecutionController>,
) -> PoolControllerImpl {
    // start operation pool
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(
        config,
        storage,
        Default::default(),
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

/// Starts the pool system
/// # Returns
/// - controller for the pool management
/// - controller for the protection management
pub fn start_pool(
    config: PoolConfig,
    storage: &Storage,
    execution_controller: Box<dyn ExecutionController>,
) -> (PoolControllerImpl, ProtectionControllerImpl) {
    let index_by_address = Arc::new(RwLock::new(PreHashMap::<Address, PoolAddrInfo>::default()));

    // start operation pool
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(
        config,
        storage,
        index_by_address.clone(),
        execution_controller.clone(),
    )));

    // start endorsement pool
    let endorsement_pool = Arc::new(RwLock::new(EndorsementPool::init(config, storage)));

    let (sender, receiver) = mpsc::channel();
    (
        PoolControllerImpl {
            _config: config,
            operation_pool: operation_pool.clone(),
            endorsement_pool,
        },
        ProtectionControllerImpl {
            thread: Some(start_protection_thread(
                operation_pool,
                index_by_address,
                execution_controller,
                receiver,
                ProtectionConfig {
                    batch_size: config.protection_batch_size,
                    roll_price: config.roll_price,
                    timeout: config.protection_time.into(),
                },
            )),
            stop_sender: sender,
        },
    )
}
