// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::settings::PoolConfig;

use super::{
    error::PoolError,
    pool_worker::{PoolCommand, PoolManagementCommand, PoolWorker},
};
use massa_logging::massa_trace;
use massa_models::{
    constants::CHANNEL_SIZE,
    prehash::{Map, Set},
    signed::Signed,
    stats::PoolStats,
    Address, BlockId, Endorsement, EndorsementId, OperationId, OperationSearchResult,
    SignedOperation, Slot,
};
use massa_protocol_exports::{ProtocolCommandSender, ProtocolPoolEventReceiver};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};
use tracing::{debug, error, info};

/// Creates a new pool controller.
///
/// # Arguments
/// * pool_settings: pool configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_pool_event_receiver: a ProtocolPoolEventReceiver instance to receive pool events from Protocol.
pub async fn start_pool_controller(
    cfg: &'static PoolConfig,
    protocol_command_sender: ProtocolCommandSender,
    protocol_pool_event_receiver: ProtocolPoolEventReceiver,
) -> Result<(PoolCommandSender, PoolManager), PoolError> {
    debug!("starting pool controller");
    massa_trace!("pool.pool_controller.start_pool_controller", {});

    // start worker
    let (command_tx, command_rx) = mpsc::channel::<PoolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<PoolManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        let res = PoolWorker::new(
            cfg,
            protocol_command_sender,
            protocol_pool_event_receiver,
            command_rx,
            manager_rx,
        )?
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("pool worker crashed: {}", err);
                Err(err)
            }
            Ok(v) => {
                info!("pool worker finished cleanly");
                Ok(v)
            }
        }
    });
    Ok((
        PoolCommandSender(command_tx),
        PoolManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct PoolCommandSender(pub mpsc::Sender<PoolCommand>);

impl PoolCommandSender {
    pub async fn add_operations(
        &mut self,
        operations: Map<OperationId, SignedOperation>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.add_operations", { "ops": operations });
        let res = self
            .0
            .send(PoolCommand::AddOperations(operations))
            .await
            .map_err(|_| PoolError::ChannelError("add_operations command send error".into()));
        res
    }

    pub async fn update_current_slot(&mut self, slot: Slot) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.update_current_slot", { "slot": slot });
        let res = self
            .0
            .send(PoolCommand::UpdateCurrentSlot(slot))
            .await
            .map_err(|_| PoolError::ChannelError("update_current_slot command send error".into()));
        res
    }

    pub async fn get_pool_stats(&mut self) -> Result<PoolStats, PoolError> {
        massa_trace!("pool.command_sender.get_pool_stats", {});
        let (response_tx, response_rx) = oneshot::channel();

        self.0
            .send(PoolCommand::GetStats(response_tx))
            .await
            .map_err(|_| PoolError::ChannelError("get_pool_stats command send error".into()))?;
        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_pool_stats {}",
                e
            ))
        })
    }

    pub async fn final_operations(
        &mut self,
        ops: Map<OperationId, (u64, u8)>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.final_operations", { "ops": ops });
        self.0
            .send(PoolCommand::FinalOperations(ops))
            .await
            .map_err(|_| PoolError::ChannelError("final_operations command send error".into()))
    }

    pub async fn update_latest_final_periods(
        &mut self,
        periods: Vec<u64>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.update_latest_final_periods", {
            "periods": periods
        });
        let res = self
            .0
            .send(PoolCommand::UpdateLatestFinalPeriods(periods))
            .await
            .map_err(|_| {
                PoolError::ChannelError("update_latest_final_periods command send error".into())
            });
        res
    }

    /// Returns a batch of operations ordered from highest to lowest rentability
    /// Return value: vector of (OperationId, Operation, operation_size: u64)
    pub async fn get_operation_batch(
        &mut self,
        target_slot: Slot,
        exclude: Set<OperationId>,
        batch_size: usize,
        max_size: u64,
    ) -> Result<Vec<(OperationId, SignedOperation, u64)>, PoolError> {
        massa_trace!("pool.command_sender.get_operation_batch", {
            "target_slot": target_slot
        });

        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(PoolCommand::GetOperationBatch {
                target_slot,
                exclude,
                batch_size,
                max_size,
                response_tx,
            })
            .await
            .map_err(|_| {
                PoolError::ChannelError("get_operation_batch command send error".into())
            })?;

        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_operation_batch {}",
                e
            ))
        })
    }

    pub async fn get_endorsements(
        &mut self,
        target_slot: Slot,
        parent: BlockId,
        creators: Vec<Address>,
    ) -> Result<Vec<(EndorsementId, Signed<Endorsement, EndorsementId>)>, PoolError> {
        massa_trace!("pool.command_sender.get_endorsements", {
            "target_slot": target_slot
        });

        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(PoolCommand::GetEndorsements {
                target_slot,
                response_tx,
                parent,
                creators,
            })
            .await
            .map_err(|_| PoolError::ChannelError("get_endorsements command send error".into()))?;

        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_endorsements {}",
                e
            ))
        })
    }

    pub async fn get_operations(
        &mut self,
        operation_ids: Set<OperationId>,
    ) -> Result<Map<OperationId, SignedOperation>, PoolError> {
        massa_trace!("pool.command_sender.get_operations", {
            "operation_ids": operation_ids
        });

        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(PoolCommand::GetOperations {
                operation_ids,
                response_tx,
            })
            .await
            .map_err(|_| PoolError::ChannelError("get_operation command send error".into()))?;

        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_operations {}",
                e
            ))
        })
    }

    pub async fn get_operations_involving_address(
        &mut self,
        address: Address,
    ) -> Result<Map<OperationId, OperationSearchResult>, PoolError> {
        massa_trace!("pool.command_sender.get_operations_involving_address", {
            "address": address
        });

        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(PoolCommand::GetRecentOperations {
                address,
                response_tx,
            })
            .await
            .map_err(|_| {
                PoolError::ChannelError(
                    "get_operations_involving_address command send error".into(),
                )
            })?;

        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_operations_involving_address {}",
                e
            ))
        })
    }

    pub async fn add_endorsements(
        &mut self,
        endorsements: Map<EndorsementId, Signed<Endorsement, EndorsementId>>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.add_endorsements", {
            "endorsements": endorsements
        });
        let res = self
            .0
            .send(PoolCommand::AddEndorsements(endorsements))
            .await
            .map_err(|_| PoolError::ChannelError("add_endorsements command send error".into()));
        res
    }

    pub async fn get_endorsements_by_address(
        &self,
        address: Address,
    ) -> Result<Map<EndorsementId, Signed<Endorsement, EndorsementId>>, PoolError> {
        massa_trace!("pool.command_sender.get_endorsements_by_address", {
            "address": address
        });

        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(PoolCommand::GetEndorsementsByAddress {
                address,
                response_tx,
            })
            .await
            .map_err(|_| {
                PoolError::ChannelError("get_endorsements_by_address command send error".into())
            })?;

        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_endorsements_by_address {:?}",
                e
            ))
        })
    }

    pub async fn get_endorsements_by_id(
        &self,
        endorsements: Set<EndorsementId>,
    ) -> Result<Map<EndorsementId, Signed<Endorsement, EndorsementId>>, PoolError> {
        massa_trace!("pool.command_sender.get_endorsements_by_id", {
            "endorsements": endorsements
        });

        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(PoolCommand::GetEndorsementsById {
                endorsements,
                response_tx,
            })
            .await
            .map_err(|_| {
                PoolError::ChannelError("get_endorsements_by_id command send error".into())
            })?;

        response_rx.await.map_err(|e| {
            PoolError::ChannelError(format!(
                "pool command response read error in get_endorsements_by_id {:?}",
                e
            ))
        })
    }
}

pub struct PoolManager {
    join_handle: JoinHandle<Result<ProtocolPoolEventReceiver, PoolError>>,
    manager_tx: mpsc::Sender<PoolManagementCommand>,
}

impl PoolManager {
    pub async fn stop(self) -> Result<ProtocolPoolEventReceiver, PoolError> {
        massa_trace!("pool.pool_controller.stop", {});
        drop(self.manager_tx);
        let protocol_pool_event_receiver = self.join_handle.await??;
        Ok(protocol_pool_event_receiver)
    }
}
