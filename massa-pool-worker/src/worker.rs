use std::{
    sync::mpsc::{sync_channel, Receiver},
    thread::JoinHandle,
};

use crate::controller_impl::{Command, PoolManagerImpl};
use crate::operation_pool::OperationPool;
use crate::{controller_impl::PoolControllerImpl, endorsement_pool::EndorsementPool};
use massa_execution_exports::ExecutionController;
use massa_pool_exports::PoolConfig;
use massa_pool_exports::{PoolController, PoolError, PoolManager};
use massa_storage::Storage;
use parking_lot::RwLock;
use std::sync::Arc;

pub(crate) struct EndorsementPoolThread(Receiver<Command>);

impl EndorsementPoolThread {
    pub(crate) fn spawn(input_mpsc: Receiver<Command>) -> JoinHandle<Result<(), PoolError>> {
        std::thread::spawn(|| {
            let this = Self(input_mpsc);
            this.run()
        })
    }

    // TODO
    fn run(self) -> Result<(), PoolError> {
        loop {
            match self.0.recv() {
                Err(_) => break,
                Ok(Command::Stop) => break,
                Ok(_) => break,
            };
        }
        Ok(())
    }
}

pub(crate) struct OperationPoolThread(Receiver<Command>);

impl OperationPoolThread {
    pub(crate) fn spawn(receiver: Receiver<Command>) -> JoinHandle<Result<(), PoolError>> {
        std::thread::spawn(|| {
            let this = Self(receiver);
            this.run()
        })
    }

    // TODO
    fn run(self) -> Result<(), PoolError> {
        loop {
            match self.0.recv() {
                Err(_) => break,
                Ok(Command::Stop) => break,
                Ok(_) => break,
            };
        }
        Ok(())
    }
}

/// Start pool manager and controller
pub fn start_pool_worker(
    config: PoolConfig,
    storage: &Storage,
    execution_controller: Box<dyn ExecutionController>,
) -> Result<(Box<dyn PoolManager>, Box<dyn PoolController>), PoolError> {
    let (operations_input_sender, operations_input_receiver) = sync_channel(config.channels_size);
    let (endorsements_input_sender, endorsements_input_receiver) =
        sync_channel(config.channels_size);
    let controller = {
        let operation_pool = Arc::new(RwLock::new(OperationPool::init(
            config,
            storage,
            execution_controller,
        )));

        let endorsement_pool = Arc::new(RwLock::new(EndorsementPool::init(config, storage)));

        PoolControllerImpl {
            _config: config,
            operation_pool,
            endorsement_pool,
            operations_input_sender: operations_input_sender.clone(),
            endorsements_input_sender: endorsements_input_sender.clone(),
        }
    };

    // launch the selector thread
    let operations_thread_handle = OperationPoolThread::spawn(operations_input_receiver);
    let endorsements_thread_handle = EndorsementPoolThread::spawn(endorsements_input_receiver);

    let manager = PoolManagerImpl {
        operations_thread_handle: Some(operations_thread_handle),
        endorsements_thread_handle: Some(endorsements_thread_handle),
        operations_input_sender,
        endorsements_input_sender,
    };
    Ok((Box::new(manager), Box::new(controller)))
}
