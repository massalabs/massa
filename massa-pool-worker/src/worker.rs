use std::{
    sync::mpsc::{sync_channel, Receiver, TryRecvError},
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

pub(crate) struct EndorsementPoolThread {
    receiver: Receiver<Command>,
    endorsement_pool: Arc<RwLock<EndorsementPool>>,
}

impl EndorsementPoolThread {
    pub(crate) fn spawn(
        receiver: Receiver<Command>,
        endorsement_pool: Arc<RwLock<EndorsementPool>>,
    ) -> JoinHandle<Result<(), PoolError>> {
        std::thread::spawn(|| {
            let this = Self {
                receiver,
                endorsement_pool,
            };
            this.run()
        })
    }

    fn run(self) -> Result<(), PoolError> {
        loop {
            match self.receiver.try_recv() {
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => break,
                Ok(Command::Stop) => break,
                Ok(Command::AddEndorsements(endorsements)) => {
                    let mut write = self.endorsement_pool.write();
                    write.add_endorsements(endorsements);
                    while let Ok(Command::AddEndorsements(endorsements)) = self.receiver.try_recv()
                    {
                        write.add_endorsements(endorsements);
                    }
                }
                Ok(Command::NotifyFinalCsPeriods(final_cs_periods)) => self
                    .endorsement_pool
                    .write()
                    .notify_final_cs_periods(&final_cs_periods),
                Ok(_) => continue,
            };
        }
        Ok(())
    }
}

pub(crate) struct OperationPoolThread {
    receiver: Receiver<Command>,
    operation_pool: Arc<RwLock<OperationPool>>,
}

impl OperationPoolThread {
    pub(crate) fn spawn(
        receiver: Receiver<Command>,
        operation_pool: Arc<RwLock<OperationPool>>,
    ) -> JoinHandle<Result<(), PoolError>> {
        std::thread::spawn(|| {
            let this = Self {
                receiver,
                operation_pool,
            };
            this.run()
        })
    }

    fn run(self) -> Result<(), PoolError> {
        loop {
            match self.receiver.try_recv() {
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => break,
                Ok(Command::Stop) => break,
                Ok(Command::AddOperations(operations)) => {
                    let mut write = self.operation_pool.write();
                    write.add_operations(operations);
                    while let Ok(Command::AddOperations(operations)) = self.receiver.try_recv() {
                        write.add_operations(operations);
                    }
                }
                Ok(Command::NotifyFinalCsPeriods(final_cs_periods)) => self
                    .operation_pool
                    .write()
                    .notify_final_cs_periods(&final_cs_periods),
                Ok(_) => continue,
            };
        }
        Ok(())
    }
}

/// Start pool manager and controller
#[allow(clippy::type_complexity)]
pub fn start_pool_controller(
    config: PoolConfig,
    storage: &Storage,
    execution_controller: Box<dyn ExecutionController>,
) -> Result<(Box<dyn PoolManager>, Box<dyn PoolController>), PoolError> {
    let (operations_input_sender, operations_input_receiver) = sync_channel(config.channels_size);
    let (endorsements_input_sender, endorsements_input_receiver) =
        sync_channel(config.channels_size);
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(
        config,
        storage,
        execution_controller,
    )));
    let endorsement_pool = Arc::new(RwLock::new(EndorsementPool::init(config, storage)));
    let controller = PoolControllerImpl {
        _config: config,
        operation_pool: operation_pool.clone(),
        endorsement_pool: endorsement_pool.clone(),
        operations_input_sender: operations_input_sender.clone(),
        endorsements_input_sender: endorsements_input_sender.clone(),
    };

    let operations_thread_handle =
        OperationPoolThread::spawn(operations_input_receiver, operation_pool);
    let endorsements_thread_handle =
        EndorsementPoolThread::spawn(endorsements_input_receiver, endorsement_pool);

    let manager = PoolManagerImpl {
        operations_thread_handle: Some(operations_thread_handle),
        endorsements_thread_handle: Some(endorsements_thread_handle),
        operations_input_sender,
        endorsements_input_sender,
    };
    Ok((Box::new(manager), Box::new(controller)))
}
