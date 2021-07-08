use std::collections::{HashMap, HashSet};

use crate::operation_pool::OperationPool;

use super::{config::PoolConfig, error::PoolError};
use communication::protocol::{
    ProtocolCommandSender, ProtocolPoolEvent, ProtocolPoolEventReceiver,
};

use models::{Operation, OperationId, SerializationContext, Slot};
use tokio::sync::{mpsc, oneshot};

/// Commands that can be proccessed by pool.
#[derive(Debug)]
pub enum PoolCommand {
    AddOperations(HashMap<OperationId, Operation>),
    UpdateCurrentSlot(Slot),
    UpdateLatestFinalPeriods(Vec<u64>),
    GetOperationBatch {
        target_slot: Slot,
        exclude: HashSet<OperationId>,
        max_count: usize,
        response_tx: oneshot::Sender<Vec<(OperationId, Operation)>>,
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
    /// serialization context
    context: SerializationContext,
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
        context: SerializationContext,
    ) -> Result<PoolWorker, PoolError> {
        massa_trace!("pool.pool_worker.new", {});
        Ok(PoolWorker {
            protocol_command_sender,
            protocol_pool_event_receiver,
            controller_command_rx,
            controller_manager_rx,
            operation_pool: OperationPool::new(cfg, thread_count, operation_validity_periods),
            context,
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
                    let context = self.context.clone();
                    self.process_pool_command(cmd, &context).await?
                },
                // receive protocol controller pool events
                evt = self.protocol_pool_event_receiver.wait_event() => {
                    massa_trace!("pool.pool_worker.run_loop.select.protocol_event", {});
                    match evt {
                        Ok(event) => {
                            let context = self.context.clone();
                            self.process_protocol_pool_event(event, &context).await?},
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
    /// * cmd: consens command to process
    async fn process_pool_command(
        &mut self,
        cmd: PoolCommand,
        context: &SerializationContext,
    ) -> Result<(), PoolError> {
        match cmd {
            PoolCommand::AddOperations(mut ops) => {
                let newly_added = self.operation_pool.add_operations(ops.clone(), context)?;
                ops.retain(|op_id, _op| newly_added.contains(op_id));
                if !ops.is_empty() {
                    self.protocol_command_sender
                        .propagate_operations(ops)
                        .await?;
                }
            }
            PoolCommand::UpdateCurrentSlot(slot) => self.operation_pool.update_current_slot(slot),
            PoolCommand::UpdateLatestFinalPeriods(periods) => {
                self.operation_pool.update_latest_final_periods(periods)
            }
            PoolCommand::GetOperationBatch {
                target_slot,
                exclude,
                max_count,
                response_tx,
            } => response_tx
                .send(
                    self.operation_pool
                        .get_operation_batch(target_slot, exclude, max_count)?,
                )
                .map_err(|e| PoolError::ChannelError(format!("could not send {:?}", e)))?,
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
        context: &SerializationContext,
    ) -> Result<(), PoolError> {
        match event {
            ProtocolPoolEvent::ReceivedOperations(mut ops) => {
                let newly_added = self.operation_pool.add_operations(ops.clone(), context)?;
                ops.retain(|op_id, _op| newly_added.contains(op_id));
                if !ops.is_empty() {
                    self.protocol_command_sender
                        .propagate_operations(ops)
                        .await?;
                }
            }
        }
        Ok(())
    }
}
