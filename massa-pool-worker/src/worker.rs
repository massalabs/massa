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
use std::sync::mpsc::TryRecvError;
use std::time::Instant;
use std::{
    sync::mpsc::{sync_channel, Receiver, RecvTimeoutError},
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
        config: PoolConfig,
    ) -> JoinHandle<()> {
        let thread_builder = thread::Builder::new().name("endorsement-pool".into());
        thread_builder
            .spawn(move || {
                let this = Self {
                    receiver,
                    endorsement_pool,
                };
                this.run(config)
            })
            .expect("failed to spawn thread : endorsement-pool")
    }

    /// Runs the thread
    fn run(self, config: PoolConfig) {
        let buffer_swap_interval = config.endorsement_pool_swap_interval.to_duration();
        let mut endorsement_pool_buffer = self.endorsement_pool.read().clone();
        let mut modified = false;
        let mut last_buffer_swap = Instant::now();
        loop {
            // try to get pending messages to process
            let mut cmd = match self.receiver.try_recv() {
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => None,
                Ok(Command::Stop) => break,
                Ok(c) => Some(c),
            };

            // Swap buffers asap if there are no messages to process.
            // This avoids waiting when we have cpu time now for swapping.
            if cmd.is_none() {
                if modified {
                    self.endorsement_pool
                        .write()
                        .replace_with(&endorsement_pool_buffer);
                    modified = false;
                }
                last_buffer_swap = Instant::now();
            }

            // wait for new command if none found
            if cmd.is_none() {
                cmd = match self.receiver.recv_timeout(buffer_swap_interval) {
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => None,
                    Ok(Command::Stop) => break,
                    Ok(c) => Some(c),
                }
            }

            match cmd {
                Some(Command::AddItems(endorsements)) => {
                    endorsement_pool_buffer.add_endorsements(endorsements);
                    modified = true;
                }
                Some(Command::NotifyFinalCsPeriods(final_cs_periods)) => {
                    endorsement_pool_buffer.notify_final_cs_periods(&final_cs_periods);
                    modified = true;
                }
                Some(_) => {
                    warn!("EndorsementPoolThread received an unexpected command");
                }
                None => {}
            }

            // On a regular basis, swap buffers if we haven't for a while.
            // This is useful under heavy congestion. Otherwise we swap as soon as the queue is empty.
            if Instant::now().saturating_duration_since(last_buffer_swap) >= buffer_swap_interval {
                if modified {
                    self.endorsement_pool
                        .write()
                        .replace_with(&endorsement_pool_buffer);
                    modified = false;
                }
                last_buffer_swap = Instant::now();
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
        let buffer_swap_interval = config.operation_pool_swap_interval.to_duration();
        let mut operation_pool_buffer = self.operation_pool.read().clone();
        let mut start_time = Instant::now();
        let tick = config.operation_pool_refresh_interval.to_duration();
        let mut modified = false;
        let mut last_buffer_swap = Instant::now();
        loop {
            // refresh if needed
            let duration = (start_time + tick).saturating_duration_since(Instant::now());
            if duration.is_zero() {
                operation_pool_buffer.refresh();
                start_time = Instant::now();
                modified = true;
            }

            // try to get pending messages to process
            let mut cmd = match self.receiver.try_recv() {
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => None,
                Ok(Command::Stop) => break,
                Ok(c) => Some(c),
            };

            // swap buffers asap if there are no messages to process
            if cmd.is_none() {
                if modified {
                    self.operation_pool
                        .write()
                        .replace_with(&operation_pool_buffer);
                    modified = false;
                }
                last_buffer_swap = Instant::now();
            }

            // wait for new command if none found
            if cmd.is_none() {
                cmd = match self
                    .receiver
                    .recv_timeout(std::cmp::min(buffer_swap_interval, duration))
                {
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => None,
                    Ok(Command::Stop) => break,
                    Ok(c) => Some(c),
                };
            }

            match cmd {
                Some(Command::AddItems(operations)) => {
                    operation_pool_buffer.add_operations(operations);
                    modified = true;
                }
                Some(Command::NotifyFinalCsPeriods(final_cs_periods)) => {
                    operation_pool_buffer.notify_final_cs_periods(&final_cs_periods);
                    modified = true;
                }
                Some(_) => {
                    warn!("OperationPoolThread received an unexpected command");
                }
                None => {}
            };

            // On a regular basis, swap buffers if we haven't for a while.
            // This is useful under heavy congestion. Otherwise we swap as soon as the queue is empty.
            if Instant::now().saturating_duration_since(last_buffer_swap) >= buffer_swap_interval {
                if modified {
                    self.operation_pool
                        .write()
                        .replace_with(&operation_pool_buffer);
                    modified = false;
                }
                last_buffer_swap = Instant::now();
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
        config: PoolConfig,
    ) -> JoinHandle<()> {
        let thread_builder = thread::Builder::new().name("denunciation-pool".into());
        thread_builder
            .spawn(move || {
                let this = Self {
                    receiver,
                    denunciation_pool,
                };
                this.run(config)
            })
            .expect("failed to spawn thread: denunciation-pool")
    }

    /// Run the thread.
    fn run(self, config: PoolConfig) {
        let buffer_swap_interval = config.denunciation_pool_swap_interval.to_duration();
        let mut denunciation_pool_buffer = self.denunciation_pool.read().clone();
        let mut start_time = Instant::now();
        let tick = config.denunciation_pool_refresh_interval.to_duration();
        let mut modified = false;
        let mut last_buffer_swap = Instant::now();
        loop {
            // refresh if needed
            let duration = (start_time + tick).saturating_duration_since(Instant::now());
            if duration.is_zero() {
                denunciation_pool_buffer.refresh_execution_state();
                start_time = Instant::now();
                modified = true;
            }

            // try to get pending messages to process
            let mut cmd = match self.receiver.try_recv() {
                Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => None,
                Ok(Command::Stop) => break,
                Ok(c) => Some(c),
            };

            // Swap buffers asap if there are no messages to process.
            // This avoids waiting when we have cpu time now for swapping.
            if cmd.is_none() {
                if modified {
                    self.denunciation_pool
                        .write()
                        .replace_with(&denunciation_pool_buffer);
                    modified = false;
                }
                last_buffer_swap = Instant::now();
            }

            // wait for new command if none found
            if cmd.is_none() {
                cmd = match self
                    .receiver
                    .recv_timeout(std::cmp::min(buffer_swap_interval, duration))
                {
                    Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => None,
                    Ok(Command::Stop) => break,
                    Ok(c) => Some(c),
                };
            }

            match cmd {
                Some(Command::AddDenunciationPrecursor(de_p)) => {
                    denunciation_pool_buffer.add_denunciation_precursor(de_p);
                    modified = true;
                }
                Some(Command::AddItems(endorsements)) => {
                    denunciation_pool_buffer.add_endorsements(endorsements);
                    modified = true;
                }
                Some(Command::NotifyFinalCsPeriods(final_cs_periods)) => {
                    denunciation_pool_buffer.notify_final_cs_periods(&final_cs_periods);
                    modified = true;
                }
                Some(_) => {
                    warn!("DenunciationPoolThread received an unexpected command");
                }
                None => {}
            }

            // On a regular basis, swap buffers if we haven't for a while.
            // This is useful under heavy congestion. Otherwise we swap as soon as the queue is empty.
            if Instant::now().saturating_duration_since(last_buffer_swap) >= buffer_swap_interval {
                if modified {
                    self.denunciation_pool
                        .write()
                        .replace_with(&denunciation_pool_buffer);
                    modified = false;
                }
                last_buffer_swap = Instant::now();
            }
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
    let denunciation_pool = Arc::new(RwLock::new(DenunciationPool::init(config, channels)));
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
        EndorsementPoolThread::spawn(endorsements_input_receiver, endorsement_pool, config);
    let denunciations_thread_handle =
        DenunciationPoolThread::spawn(denunciations_input_receiver, denunciation_pool, config);

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
