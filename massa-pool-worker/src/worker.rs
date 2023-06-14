//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Write worker for the pools, allowing asynchronous writes.

use crate::controller_impl::{Command, PoolManagerImpl};
use crate::denunciation_pool::DenunciationPool;
use crate::operation_pool::OperationPool;
use crate::{controller_impl::PoolControllerImpl, endorsement_pool::EndorsementPool};
use massa_pool_exports::PoolConfig;
use massa_pool_exports::{PoolChannels, PoolController, PoolManager};
use massa_storage::Storage;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::time::Instant;
use std::{
    sync::mpsc::{sync_channel, Receiver, RecvError, RecvTimeoutError},
    sync::Arc,
    thread,
    thread::JoinHandle,
};
use tracing::warn;

/// Endorsement pool write thread instance
pub(crate) struct EndorsementPoolThread {
    /// Command reception channel
    receiver: Receiver<Command>,
    /// Shared reference to the pool
    endorsement_pool: Arc<RwLock<EndorsementPool>>,
}

impl EndorsementPoolThread {
    /// Spawns a pool writer thread, returning a join handle.
    pub(crate) fn spawn(
        receiver: Receiver<Command>,
        endorsement_pool: Arc<RwLock<EndorsementPool>>,
    ) -> JoinHandle<()> {
        let thread_builder = thread::Builder::new().name("endorsement-pool".into());
        thread_builder
            .spawn(|| {
                let this = Self {
                    receiver,
                    endorsement_pool,
                };
                this.run()
            })
            .expect("failed to spawn thread : endorsement-pool")
    }

    /// Runs the thread
    fn run(self) {
        loop {
            match self.receiver.recv() {
                Err(RecvError) => break,
                Ok(Command::Stop) => {
                    break;
                }
                Ok(Command::AddItems(endorsements)) => {
                    self.endorsement_pool.write().add_endorsements(endorsements)
                }
                Ok(Command::NotifyFinalCsPeriods(final_cs_periods)) => self
                    .endorsement_pool
                    .write()
                    .notify_final_cs_periods(&final_cs_periods),
                _ => {
                    warn!("EndorsementPoolThread received an unexpected command");
                    continue;
                }
            }
        }
    }
}

/// Operation pool writer thread.
pub(crate) struct OperationPoolThread {
    /// Command reception channel
    receiver: Receiver<Command>,
    /// Shared reference to the operation pool
    operation_pool: Arc<RwLock<OperationPool>>,
}

impl OperationPoolThread {
    /// Spawns a pool writer thread, returning a join handle.
    pub(crate) fn spawn(
        receiver: Receiver<Command>,
        operation_pool: Arc<RwLock<OperationPool>>,
        config: PoolConfig,
    ) -> JoinHandle<()> {
        let thread_builder = thread::Builder::new().name("operation-pool".into());
        thread_builder
            .spawn(move || {
                let this = Self {
                    receiver,
                    operation_pool,
                };
                this.run(config)
            })
            .expect("failed to spawn thread: operation-pool")
    }

    /// Run the thread.
    fn run(self, config: PoolConfig) {
        let mut next_refresh = Instant::now();
        loop {
            match self.receiver.recv_deadline(next_refresh) {
                Err(RecvTimeoutError::Disconnected) | Ok(Command::Stop) => break,
                Ok(Command::AddItems(operations)) => {
                    self.operation_pool.write().add_operations(operations)
                }
                Ok(Command::NotifyFinalCsPeriods(final_cs_periods)) => self
                    .operation_pool
                    .write()
                    .notify_final_cs_periods(&final_cs_periods),
                Ok(_) => {
                    warn!("OperationPoolThread received an unexpected command");
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
            };
            if next_refresh <= Instant::now() {
                self.operation_pool.write().refresh();
                next_refresh = Instant::now()
                    .checked_add(config.operation_pool_refresh_interval.to_duration())
                    .expect("could not compute time of next op pool refresh")
            }
        }
    }
}

/// Denunciation pool writer thread.
pub(crate) struct DenunciationPoolThread {
    /// Command reception channel
    receiver: Receiver<Command>,
    /// Shared reference to the denunciation pool
    denunciation_pool: Arc<RwLock<DenunciationPool>>,
}

impl DenunciationPoolThread {
    /// Spawns a pool writer thread, returning a join handle.
    pub(crate) fn spawn(
        receiver: Receiver<Command>,
        denunciation_pool: Arc<RwLock<DenunciationPool>>,
    ) -> JoinHandle<()> {
        let thread_builder = thread::Builder::new().name("denunciation-pool".into());
        thread_builder
            .spawn(|| {
                let this = Self {
                    receiver,
                    denunciation_pool,
                };
                this.run()
            })
            .expect("failed to spawn thread : denunciation-pool")
    }

    /// Run the thread.
    fn run(self) {
        loop {
            match self.receiver.recv() {
                Err(RecvError) => {
                    break;
                }
                Ok(Command::Stop) => {
                    break;
                }
                Ok(Command::AddDenunciationPrecursor(de_p)) => self
                    .denunciation_pool
                    .write()
                    .add_denunciation_precursor(de_p),
                Ok(Command::AddItems(endorsements)) => self
                    .denunciation_pool
                    .write()
                    .add_endorsements(endorsements),
                Ok(Command::NotifyFinalCsPeriods(final_cs_periods)) => self
                    .denunciation_pool
                    .write()
                    .notify_final_cs_periods(&final_cs_periods),
            };
        }
    }
}

/// Start pool manager and controller
#[allow(clippy::type_complexity)]
pub fn start_pool_controller(
    config: PoolConfig,
    storage: &Storage,
    channels: PoolChannels,
    wallet: Arc<RwLock<Wallet>>,
) -> (Box<dyn PoolManager>, Box<dyn PoolController>) {
    let (operations_input_sender, operations_input_receiver) =
        sync_channel(config.operations_channel_size);
    let (endorsements_input_sender, endorsements_input_receiver) =
        sync_channel(config.endorsements_channel_size);
    let (denunciations_input_sender, denunciations_input_receiver) =
        sync_channel(config.denunciations_channel_size);
    let operation_pool = Arc::new(RwLock::new(OperationPool::init(
        config,
        storage,
        channels.clone(),
        wallet.clone(),
    )));
    let endorsement_pool = Arc::new(RwLock::new(EndorsementPool::init(
        config,
        storage,
        channels.clone(),
        wallet,
    )));
    let denunciation_pool = Arc::new(RwLock::new(DenunciationPool::init(
        config,
        channels.clone(),
    )));
    let controller = PoolControllerImpl {
        _config: config,
        operation_pool: operation_pool.clone(),
        endorsement_pool: endorsement_pool.clone(),
        denunciation_pool: denunciation_pool.clone(),
        operations_input_sender: operations_input_sender.clone(),
        endorsements_input_sender: endorsements_input_sender.clone(),
        denunciations_input_sender: denunciations_input_sender.clone(),
        last_cs_final_periods: vec![0u64; usize::from(config.thread_count)],
    };

    let operations_thread_handle =
        OperationPoolThread::spawn(operations_input_receiver, operation_pool, config);
    let endorsements_thread_handle =
        EndorsementPoolThread::spawn(endorsements_input_receiver, endorsement_pool);
    let denunciations_thread_handle =
        DenunciationPoolThread::spawn(denunciations_input_receiver, denunciation_pool);

    let manager = PoolManagerImpl {
        operations_thread_handle: Some(operations_thread_handle),
        endorsements_thread_handle: Some(endorsements_thread_handle),
        denunciations_thread_handle: Some(denunciations_thread_handle),
        operations_input_sender,
        endorsements_input_sender,
        denunciations_input_sender,
    };
    (Box::new(manager), Box::new(controller))
}
