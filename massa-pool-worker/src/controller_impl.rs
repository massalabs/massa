// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Pool controller implementation

use massa_models::denunciation::DenunciationPrecursor;
use massa_models::{
    block_id::BlockId, endorsement::EndorsementId, operation::OperationId, slot::Slot,
};
use massa_pool_exports::{PoolConfig, PoolController, PoolManager};
use massa_storage::Storage;
use parking_lot::RwLock;
use std::sync::mpsc::TrySendError;
use std::sync::{mpsc::SyncSender, Arc};
use tracing::{info, warn};

use crate::{
    denunciation_pool::DenunciationPool, endorsement_pool::EndorsementPool,
    operation_pool::OperationPool,
};

/// A generic command to send commands to a pool
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Add items to the pool
    AddItems(Storage),
    /// Add denunciation to the pool
    // AddDenunciation(Denunciation),
    /// Add denunciation precursor to the pool
    AddDenunciationPrecursor(DenunciationPrecursor),
    /// Notify of new final consensus periods
    NotifyFinalCsPeriods(Vec<u64>),
    /// Stop the worker
    Stop,
}

/// Pool controller
#[derive(Clone)]
pub struct PoolControllerImpl {
    /// Config
    pub(crate) _config: PoolConfig,
    /// Shared reference to the operation pool
    pub(crate) operation_pool: Arc<RwLock<OperationPool>>,
    /// Shared reference to the endorsement pool
    pub(crate) endorsement_pool: Arc<RwLock<EndorsementPool>>,
    /// Shared reference to the denunciation pool
    pub(crate) denunciation_pool: Arc<RwLock<DenunciationPool>>,
    /// Operation write worker command sender
    pub(crate) operations_input_sender: SyncSender<Command>,
    /// Endorsement write worker command sender
    pub(crate) endorsements_input_sender: SyncSender<Command>,

    /// Denunciation write worker command sender
    pub(crate) denunciations_input_sender: SyncSender<Command>,
    // /// Denunciation precursor sender
    // pub(crate) denunciation_precursor_sender: SyncSender<Command>,
    /// Last final periods from Consensus
    pub last_cs_final_periods: Vec<u64>,
}

impl PoolController for PoolControllerImpl {
    /// Asynchronously add operations to pool. Simply print a warning on failure.
    fn add_operations(&mut self, ops: Storage) {
        match self
            .operations_input_sender
            .try_send(Command::AddItems(ops))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!("Could not add operations to pool: worker is unreachable.");
            }
            Err(TrySendError::Full(_)) => {
                warn!("Could not add operations to pool: worker channel is full.");
            }
            Ok(_) => {}
        }
    }

    /// Asynchronously add endorsements to pool. Simply print a warning on failure.
    fn add_endorsements(&mut self, endorsements: Storage) {
        match self
            .endorsements_input_sender
            .try_send(Command::AddItems(endorsements))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!("Could not add endorsements to pool: worker is unreachable.");
            }
            Err(TrySendError::Full(_)) => {
                warn!("Could not add endorsements to pool: worker channel is full.");
            }
            Ok(_) => {}
        }
    }

    /// Asynchronously notify of new final consensus periods. Simply print a warning on failure.
    fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        self.last_cs_final_periods = final_cs_periods.to_vec();

        match self
            .operations_input_sender
            .try_send(Command::NotifyFinalCsPeriods(final_cs_periods.to_vec()))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!("Could not notify operation pool of new final slots: worker is unreachable.");
            }
            Err(TrySendError::Full(_)) => {
                warn!(
                    "Could not notify operation pool of new final slots: worker channel is full."
                );
            }
            Ok(_) => {}
        }

        match self
            .endorsements_input_sender
            .try_send(Command::NotifyFinalCsPeriods(final_cs_periods.to_vec()))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!(
                    "Could not notify endorsement pool of new final slots: worker is unreachable."
                );
            }
            Err(TrySendError::Full(_)) => {
                warn!(
                    "Could not notify endorsement pool of new final slots: worker channel is full."
                );
            }
            Ok(_) => {}
        }

        match self
            .denunciations_input_sender
            .try_send(Command::NotifyFinalCsPeriods(final_cs_periods.to_vec()))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!(
                    "Could not notify endorsement pool of new final slots: worker is unreachable."
                );
            }
            Err(TrySendError::Full(_)) => {
                warn!(
                    "Could not notify endorsement pool of new final slots: worker channel is full."
                );
            }
            Ok(_) => {}
        }
    }

    /// get operations for block creation
    fn get_block_operations(&self, slot: &Slot) -> (Vec<OperationId>, Storage) {
        self.operation_pool.read().get_block_operations(slot)
    }

    /// get endorsements for a block
    fn get_block_endorsements(
        &self,
        target_block: &BlockId,
        target_slot: &Slot,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        self.endorsement_pool
            .read()
            .get_block_endorsements(target_slot, target_block)
    }

    /// Returns a boxed clone of self.
    /// Allows cloning `Box<dyn PoolController>`,
    fn clone_box(&self) -> Box<dyn PoolController> {
        Box::new(self.clone())
    }

    /// Get the number of endorsements in the pool
    fn get_endorsement_count(&self) -> usize {
        self.endorsement_pool.read().len()
    }

    /// Get the number of operations in the pool
    fn get_operation_count(&self) -> usize {
        self.operation_pool.read().len()
    }

    /// Check if the pool contains a list of endorsements. Returns one boolean per item.
    fn contains_endorsements(&self, endorsements: &[EndorsementId]) -> Vec<bool> {
        let lck = self.endorsement_pool.read();
        endorsements.iter().map(|id| lck.contains(id)).collect()
    }

    /// Check if the pool contains a list of operations. Returns one boolean per item.
    fn contains_operations(&self, operations: &[OperationId]) -> Vec<bool> {
        let lck = self.operation_pool.read();
        operations.iter().map(|id| lck.contains(id)).collect()
    }

    /// Add denunciation precursor to pool
    fn add_denunciation_precursor(&self, denunciation_precursor: DenunciationPrecursor) {
        match self
            .denunciations_input_sender
            .try_send(Command::AddDenunciationPrecursor(denunciation_precursor))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!("Could not add denunciation precursor to pool: worker is unreachable.");
            }
            Err(TrySendError::Full(_)) => {
                warn!("Could not add denunciation precursor to pool: worker channel is full.");
            }
            Ok(_) => {}
        }
    }

    /// Add denunciation to pool
    /*
    fn add_denunciation(&mut self, denunciation: Denunciation) {
        match self
            .denunciations_input_sender
            .try_send(Command::AddDenunciation(denunciation))
        {
            Err(TrySendError::Disconnected(_)) => {
                warn!("Could not add denunciations to pool: worker is unreachable.");
            }
            Err(TrySendError::Full(_)) => {
                warn!("Could not add denunciations to pool: worker channel is full.");
            }
            Ok(_) => {}
        }
    }
    */

    /// Get the number of denunciations in the pool
    fn get_denunciation_count(&self) -> usize {
        self.denunciation_pool.read().len()
    }

    /// Get final consensus periods
    fn get_final_cs_periods(&self) -> &Vec<u64> {
        &self.last_cs_final_periods
    }
}

/// Implementation of the pool manager.
///
/// Contains the operations and endorsements thread handles.
pub struct PoolManagerImpl {
    /// Handle used to join the operation thread
    pub(crate) operations_thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Handle used to join the endorsement thread
    pub(crate) endorsements_thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Handle used to join the denunciation thread
    pub(crate) denunciations_thread_handle: Option<std::thread::JoinHandle<()>>,
    /// Operations input data mpsc (used to stop the pool thread)
    pub(crate) operations_input_sender: SyncSender<Command>,
    /// Endorsements input data mpsc (used to stop the pool thread)
    pub(crate) endorsements_input_sender: SyncSender<Command>,
    /// Denunciations input data mpsc (used to stop the pool thread)
    pub(crate) denunciations_input_sender: SyncSender<Command>,
}

impl PoolManager for PoolManagerImpl {
    /// Stops the worker
    fn stop(&mut self) {
        info!("stopping pool workers...");
        let _ = self.operations_input_sender.send(Command::Stop);
        let _ = self.endorsements_input_sender.send(Command::Stop);
        let _ = self.denunciations_input_sender.send(Command::Stop);
        if let Some(join_handle) = self.operations_thread_handle.take() {
            join_handle
                .join()
                .expect("operations pool thread panicked on try to join");
        }
        if let Some(join_handle) = self.endorsements_thread_handle.take() {
            join_handle
                .join()
                .expect("endorsements pool thread panicked on try to join");
        }
        if let Some(join_handle) = self.denunciations_thread_handle.take() {
            join_handle
                .join()
                .expect("denunciations pool thread panicked on try to join");
        }
        info!("pool workers stopped");
    }
}
