// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::error::PoolError;
use crate::operation_pool::OperationPool;
use crate::{endorsement_pool::EndorsementPool, settings::PoolConfig};
use massa_models::prehash::{Map, Set};
use massa_models::signed::Signed;
use massa_models::stats::PoolStats;
use massa_models::{
    Address, BlockId, Endorsement, EndorsementId, Operation, OperationId,
    OperationSearchResult, Slot,
};
use massa_protocol_exports::{ProtocolCommandSender, ProtocolPoolEvent, ProtocolPoolEventReceiver};
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

/// Commands that can be processed by pool.
#[derive(Debug)]
pub enum PoolCommand {
    AddOperations(Map<OperationId, Operation>),
    UpdateCurrentSlot(Slot),
    UpdateLatestFinalPeriods(Vec<u64>),
    GetOperationBatch {
        target_slot: Slot,
        exclude: Set<OperationId>,
        batch_size: usize,
        max_size: u64,
        response_tx: oneshot::Sender<Vec<(OperationId, Operation, u64)>>,
    },
    GetOperations {
        operation_ids: Set<OperationId>,
        response_tx: oneshot::Sender<Map<OperationId, Operation>>,
    },
    GetRecentOperations {
        address: Address,
        response_tx: oneshot::Sender<Map<OperationId, OperationSearchResult>>,
    },
    FinalOperations(Map<OperationId, (u64, u8)>), // (end of validity period, thread)
    GetEndorsements {
        target_slot: Slot,
        parent: BlockId,
        creators: Vec<Address>,
        response_tx:
            oneshot::Sender<Vec<(EndorsementId, Signed<Endorsement, EndorsementId>)>>,
    },
    AddEndorsements(Map<EndorsementId, Signed<Endorsement, EndorsementId>>),
    GetStats(oneshot::Sender<PoolStats>),
    GetEndorsementsByAddress {
        address: Address,
        response_tx: oneshot::Sender<Map<EndorsementId, Signed<Endorsement, EndorsementId>>>,
    },
    GetEndorsementsById {
        endorsements: Set<EndorsementId>,
        response_tx: oneshot::Sender<Map<EndorsementId, Signed<Endorsement, EndorsementId>>>,
    },
}

/// Events that are emitted by pool.
#[derive(Debug, Clone)]
pub enum PoolManagementCommand {}

/// Manages pool.
pub struct PoolWorker {
    /// Associated protocol command sender.
    protocol_command_sender: ProtocolCommandSender,
    /// Associated protocol pool event listener.
    protocol_pool_event_receiver: ProtocolPoolEventReceiver,
    /// Channel receiving pool commands.
    controller_command_rx: mpsc::Receiver<PoolCommand>,
    /// Channel receiving pool management commands.
    controller_manager_rx: mpsc::Receiver<PoolManagementCommand>,
    /// operation pool
    operation_pool: OperationPool,
    /// Endorsement pool.
    endorsement_pool: EndorsementPool,
}

impl PoolWorker {
    /// Creates a new pool controller.
    /// Initiates the random selector.
    ///
    /// # Arguments
    /// * pool_settings: pool configuration.
    /// * thread_count: number of threads
    /// * operation_validity_periods : operation validity period
    /// * protocol_command_sender: associated protocol controller
    /// * protocol_command_sender protocol pool event receiver
    /// * controller_command_rx: Channel receiving pool commands.
    /// * controller_manager_rx: Channel receiving pool management commands.
    pub fn new(
        cfg: &'static PoolConfig,
        protocol_command_sender: ProtocolCommandSender,
        protocol_pool_event_receiver: ProtocolPoolEventReceiver,
        controller_command_rx: mpsc::Receiver<PoolCommand>,
        controller_manager_rx: mpsc::Receiver<PoolManagementCommand>,
    ) -> Result<PoolWorker, PoolError> {
        massa_trace!("pool.pool_worker.new", {});
        Ok(PoolWorker {
            protocol_command_sender,
            protocol_pool_event_receiver,
            controller_command_rx,
            controller_manager_rx,
            operation_pool: OperationPool::new(cfg),
            endorsement_pool: EndorsementPool::new(cfg),
        })
    }

    /// Pool work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<ProtocolPoolEventReceiver, PoolError> {
        loop {
            massa_trace!("pool.pool_worker.run_loop.select", {});
            /*
                select! without the "biased" modifier will randomly select the 1st branch to check,
                then will check the next ones in the order they are written.
                We choose this order:
                    * manager commands: low freq, avoid having to wait to stop
                    * pool commands (low to medium freq): respond quickly to consensus to avoid blocking it
                    * protocol commands (high frequency): process incoming protocol objects
            */
            tokio::select! {
                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => {
                    massa_trace!("pool.pool_worker.run_loop.select.manager", {});
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}

                // listen pool commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    massa_trace!("pool.pool_worker.run_loop.pool_command", {});
                    self.process_pool_command(cmd).await?
                },

                // receive protocol controller pool events
                evt = self.protocol_pool_event_receiver.wait_event() => {
                    massa_trace!("pool.pool_worker.run_loop.select.protocol_event", {});
                    match evt {
                        Ok(event) => {
                            self.process_protocol_pool_event(event).await?},
                        Err(err) => return Err(PoolError::ProtocolError(Box::new(err)))
                    }
                },
            }
        }
        // end loop
        Ok(self.protocol_pool_event_receiver)
    }

    /// Manages given pool command.
    ///
    /// # Argument
    /// * cmd: consensus command to process
    async fn process_pool_command(&mut self, cmd: PoolCommand) -> Result<(), PoolError> {
        match cmd {
            PoolCommand::AddOperations(mut operations) => {
                let newly_added = self.operation_pool.add_operations(operations.clone())?;
                operations.retain(|op_id, _op| newly_added.contains(op_id));
                if !operations.is_empty() {
                    self.protocol_command_sender
                        .propagate_operations(operations)
                        .await?;
                }
            }
            PoolCommand::UpdateCurrentSlot(slot) => {
                self.operation_pool.update_current_slot(slot);
                self.endorsement_pool.update_current_slot(slot)
            }
            PoolCommand::UpdateLatestFinalPeriods(periods) => {
                self.operation_pool
                    .update_latest_final_periods(periods.clone())?;
                self.endorsement_pool.update_latest_final_periods(periods)
            }
            PoolCommand::GetOperationBatch {
                target_slot,
                exclude,
                batch_size,
                max_size,
                response_tx,
            } => {
                if response_tx
                    .send(self.operation_pool.get_operation_batch(
                        target_slot,
                        exclude,
                        batch_size,
                        max_size,
                    )?)
                    .is_err()
                {
                    warn!("pool: could not send get_operation_batch response");
                }
            }
            PoolCommand::GetOperations {
                operation_ids,
                response_tx,
            } => {
                if response_tx
                    .send(self.operation_pool.get_operations(&operation_ids))
                    .is_err()
                {
                    warn!("pool: could not send get_operations response");
                }
            }
            PoolCommand::GetRecentOperations {
                address,
                response_tx,
            } => {
                if response_tx
                    .send(
                        self.operation_pool
                            .get_operations_involving_address(&address)?,
                    )
                    .is_err()
                {
                    warn!("pool: could not send get_operations_involving_address response");
                }
            }
            PoolCommand::FinalOperations(ops) => self.operation_pool.new_final_operations(ops)?,
            PoolCommand::GetEndorsements {
                target_slot,
                parent,
                creators,
                response_tx,
            } => {
                if response_tx
                    .send(
                        self.endorsement_pool
                            .get_endorsements(target_slot, parent, creators)?,
                    )
                    .is_err()
                {
                    warn!("pool: could not send get_endorsements response");
                }
            }
            PoolCommand::AddEndorsements(mut endorsements) => {
                let newly_added = self
                    .endorsement_pool
                    .add_endorsements(endorsements.clone())?;
                endorsements.retain(|e_id, _e| newly_added.contains(e_id));
                if !endorsements.is_empty() {
                    self.protocol_command_sender
                        .propagate_endorsements(endorsements)
                        .await?;
                }
            }
            PoolCommand::GetStats(response_tx) => {
                if response_tx
                    .send(PoolStats {
                        operation_count: self.operation_pool.len() as u64,
                        endorsement_count: self.endorsement_pool.len() as u64,
                    })
                    .is_err()
                {
                    warn!("pool: could not send PoolStats response");
                }
            }
            PoolCommand::GetEndorsementsByAddress {
                response_tx,
                address,
            } => {
                if response_tx
                    .send(self.endorsement_pool.get_endorsement_by_address(address)?)
                    .is_err()
                {
                    warn!("pool: could not send PoolStats response");
                }
            }
            PoolCommand::GetEndorsementsById {
                response_tx,
                endorsements,
            } => {
                if response_tx
                    .send(self.endorsement_pool.get_endorsement_by_id(endorsements))
                    .is_err()
                {
                    warn!("pool: could not send PoolStats response");
                }
            }
        }
        Ok(())
    }

    /// Manages received protocol pool events.
    ///
    /// # Arguments
    /// * event: event type to process.
    async fn process_protocol_pool_event(
        &mut self,
        event: ProtocolPoolEvent,
    ) -> Result<(), PoolError> {
        match event {
            ProtocolPoolEvent::ReceivedOperations {
                mut operations,
                propagate,
            } => {
                if propagate {
                    let newly_added = self.operation_pool.add_operations(operations.clone())?;
                    operations.retain(|op_id, _op| newly_added.contains(op_id));
                    if !operations.is_empty() {
                        self.protocol_command_sender
                            .propagate_operations(operations)
                            .await?;
                    }
                } else {
                    self.operation_pool.add_operations(operations)?;
                }
            }
            ProtocolPoolEvent::ReceivedEndorsements {
                mut endorsements,
                propagate,
            } => {
                if propagate {
                    let newly_added = self
                        .endorsement_pool
                        .add_endorsements(endorsements.clone())?;
                    endorsements.retain(|id, _| newly_added.contains(id));
                    if !endorsements.is_empty() {
                        self.protocol_command_sender
                            .propagate_endorsements(endorsements)
                            .await?;
                    }
                } else {
                    self.endorsement_pool.add_endorsements(endorsements)?;
                }
            }
        }
        Ok(())
    }
}
