use std::collections::HashSet;

use super::{
    config::{PoolConfig, CHANNEL_SIZE},
    error::PoolError,
    pool_worker::{PoolCommand, PoolManagementCommand, PoolWorker},
};
use communication::protocol::{ProtocolCommandSender, ProtocolPoolEventReceiver};
use logging::{debug, massa_trace};
use models::{Operation, OperationId, SerializationContext, Slot};
use tokio::{
    sync::{mpsc, oneshot::Sender},
    task::JoinHandle,
};

/// Creates a new pool controller.
///
/// # Arguments
/// * cfg: pool configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_pool_event_receiver: a ProtocolPoolEventReceiver instance to receive pool events from Protocol.
pub async fn start_pool_controller(
    cfg: PoolConfig,
    thread_count: u8,
    protocol_command_sender: ProtocolCommandSender,
    protocol_pool_event_receiver: ProtocolPoolEventReceiver,
    context: SerializationContext,
) -> Result<(PoolCommandSender, PoolManager), PoolError> {
    debug!("starting pool controller");
    massa_trace!("pool.pool_controller.start_pool_controller", {});

    // start worker
    let (command_tx, command_rx) = mpsc::channel::<PoolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<PoolManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = PoolWorker::new(
            cfg_copy,
            thread_count,
            protocol_command_sender,
            protocol_pool_event_receiver,
            command_rx,
            manager_rx,
            context,
        )?
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("pool worker crashed: {:?}", err);
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
        ops: Vec<(OperationId, Operation)>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.add_operations", { "ops": ops });
        let res = self
            .0
            .send(PoolCommand::AddOperations(ops))
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

    pub async fn update_latest_final_periods(
        &mut self,
        periods: Vec<u64>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.update_latest_final_periods", {
            "ops": periods
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

    pub async fn get_operation_batch(
        &mut self,
        target_slot: Slot,
        exclude: HashSet<OperationId>,
        max_count: usize,
        response_tx: Sender<Vec<(OperationId, Operation)>>,
    ) -> Result<(), PoolError> {
        massa_trace!("pool.command_sender.get_operation_batch", {
            "target_slot": target_slot
        });
        let res = self
            .0
            .send(PoolCommand::GetOperationBatch {
                target_slot,
                exclude,
                max_count,
                response_tx,
            })
            .await
            .map_err(|_| PoolError::ChannelError("get_operation_batch command send error".into()));
        res
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
