// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::{
    collections::{HashMap, HashSet},
    u64,
};

use crate::endorsement_pool::EndorsementPool;
use crate::operation_pool::OperationPool;

use super::{config::PoolConfig, error::PoolError};
use communication::protocol::{
    ProtocolCommandSender, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};
use models::{
    Address, BlockId, Endorsement, EndorsementId, Operation, OperationId, OperationSearchResult,
    Slot,
};
use tokio::sync::{mpsc, oneshot};

/// Commands that can be processed by pool.
#[derive(Debug)]
pub enum PoolCommand {
    AddOperations(HashMap<OperationId, Operation>),
    UpdateCurrentSlot(Slot),
    UpdateLatestFinalPeriods(Vec<u64>),
    GetOperationBatch {
        target_slot: Slot,
        exclude: HashSet<OperationId>,
        batch_size: usize,
        max_size: u64,
        response_tx: oneshot::Sender<Vec<(OperationId, Operation, u64)>>,
    },
    GetOperations {
        operation_ids: HashSet<OperationId>,
        response_tx: oneshot::Sender<HashMap<OperationId, Operation>>,
    },
    GetRecentOperations {
        address: Address,
        response_tx: oneshot::Sender<HashMap<OperationId, OperationSearchResult>>,
    },
    FinalOperations(HashMap<OperationId, (u64, u8)>), // (end of validity period, thread)
    GetEndorsements {
        target_slot: Slot,
        parent: BlockId,
        creators: Vec<Address>,
        response_tx: oneshot::Sender<Vec<Endorsement>>,
    },
    AddEndorsements(HashMap<EndorsementId, Endorsement>),
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
    /// * cfg: pool configuration.
    /// * thread_count: number of threads
    /// * operation_validity_periods : operation validity period
    /// * protocol_command_sender: associated protocol controller
    /// * protocol_command_sender protocol pool event receiver
    /// * controller_command_rx: Channel receiving pool commands.
    /// * controller_manager_rx: Channel receiving pool management commands.
    pub fn new(
        cfg: PoolConfig,
        thread_count: u8,
        operation_validity_periods: u64,
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
            operation_pool: OperationPool::new(
                cfg.clone(),
                thread_count,
                operation_validity_periods,
            ),
            endorsement_pool: EndorsementPool::new(cfg),
        })
    }

    /// Pool work is managed here.
    /// It's mostly a tokio::select within a loop.
    pub async fn run_loop(mut self) -> Result<ProtocolPoolEventReceiver, PoolError> {
        loop {
            massa_trace!("pool.pool_worker.run_loop.select", {});
            tokio::select! {
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
                        Err(err) => return Err(PoolError::CommunicationError(err))
                    }
                },
                // listen to manager commands
                cmd = self.controller_manager_rx.recv() => {
                    massa_trace!("pool.pool_worker.run_loop.select.manager", {});
                    match cmd {
                    None => break,
                    Some(_) => {}
                }}
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
                self.operation_pool.update_current_slot(slot)?
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
            } => response_tx
                .send(self.operation_pool.get_operation_batch(
                    target_slot,
                    exclude,
                    batch_size,
                    max_size,
                )?)
                .map_err(|e| PoolError::ChannelError(format!("could not send {:?}", e)))?,
            PoolCommand::GetOperations {
                operation_ids,
                response_tx,
            } => response_tx
                .send(self.operation_pool.get_operations(&operation_ids))
                .map_err(|e| PoolError::ChannelError(format!("could not send {:?}", e)))?,
            PoolCommand::GetRecentOperations {
                address,
                response_tx,
            } => response_tx
                .send(
                    self.operation_pool
                        .get_operations_involving_address(&address)?,
                )
                .map_err(|e| PoolError::ChannelError(format!("could not send {:?}", e)))?,
            PoolCommand::FinalOperations(ops) => self.operation_pool.new_final_operations(ops)?,
            PoolCommand::GetEndorsements {
                target_slot,
                parent,
                creators,
                response_tx,
            } => response_tx
                .send(
                    self.endorsement_pool
                        .get_endorsements(target_slot, parent, creators)?,
                )
                .map_err(|e| PoolError::ChannelError(format!("could not send {:?}", e)))?,
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
