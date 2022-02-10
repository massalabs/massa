use crate::error::ExecutionError;
use crate::settings::ExecutionConfigs;
use crate::worker::{
    ExecutionCommand, ExecutionEvent, ExecutionManagementCommand, ExecutionWorker,
};
use crate::BootstrapExecutionState;
use massa_models::{
    api::SCELedgerInfo, constants::CHANNEL_SIZE, execution::ExecuteReadOnlyResponse,
    output_event::SCOutputEvent, prehash::Map, Address, Amount, Block, BlockId, OperationId, Slot,
};
use std::collections::VecDeque;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{error, info};

/// A sender of execution commands.
#[derive(Clone)]
pub struct ExecutionCommandSender(pub mpsc::Sender<ExecutionCommand>);

/// A receiver of execution events.
pub struct ExecutionEventReceiver(pub mpsc::UnboundedReceiver<ExecutionEvent>);

impl ExecutionEventReceiver {
    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ExecutionEvent> {
        let mut remaining_events: VecDeque<ExecutionEvent> = VecDeque::new();

        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

/// A sender of execution management commands.
pub struct ExecutionManager {
    join_handle: JoinHandle<Result<(), ExecutionError>>,
    manager_tx: mpsc::Sender<ExecutionManagementCommand>,
}

impl ExecutionManager {
    pub async fn stop(self) -> Result<(), ExecutionError> {
        drop(self.manager_tx);
        if let Err(err) = self.join_handle.await {
            error!("execution worker crashed: {}", err);
            return Err(ExecutionError::JoinError);
        };

        info!("execution worker finished cleanly");
        Ok(())
    }
}

/// Creates a new execution controller.
///
/// # Arguments
/// * cfg: execution configuration
/// * bootstrap_state: optional bootstrap state
///
/// TODO: add a consensus command sender,
/// to be able to send the `TransferToConsensus` message.
pub async fn start_controller(
    cfg: ExecutionConfigs,
    bootstrap_state: Option<BootstrapExecutionState>,
) -> Result<
    (
        ExecutionCommandSender,
        ExecutionEventReceiver,
        ExecutionManager,
    ),
    ExecutionError,
> {
    let (command_tx, command_rx) = mpsc::channel::<ExecutionCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ExecutionManagementCommand>(1);

    // Unbounded, as execution is limited per metering already.
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ExecutionEvent>();
    let worker = ExecutionWorker::new(cfg, event_tx, command_rx, manager_rx, bootstrap_state)?;
    let join_handle = tokio::spawn(async move {
        match worker.run_loop().await {
            Err(err) => Err(err),
            Ok(v) => Ok(v),
        }
    });
    Ok((
        ExecutionCommandSender(command_tx),
        ExecutionEventReceiver(event_rx),
        ExecutionManager {
            join_handle,
            manager_tx,
        },
    ))
}

impl ExecutionCommandSender {
    /// notify of a blockclique change
    pub async fn update_blockclique(
        &self,
        finalized_blocks: Map<BlockId, Block>,
        blockclique: Map<BlockId, Block>,
    ) -> Result<(), ExecutionError> {
        self.0
            .send(ExecutionCommand::BlockCliqueChanged {
                blockclique,
                finalized_blocks,
            })
            .await
            .map_err(|_err| {
                ExecutionError::ChannelError(
                    "could not send BlockCliqueChanged command to execution".into(),
                )
            })?;
        Ok(())
    }

    pub async fn get_bootstrap_state(&self) -> Result<BootstrapExecutionState, ExecutionError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(ExecutionCommand::GetBootstrapState(response_tx))
            .await
            .map_err(|_| {
                ExecutionError::ChannelError("could not send GetBootstrapState command".into())
            })?;
        response_rx.await.map_err(|_| {
            ExecutionError::ChannelError("could not send GetBootstrapState upstream".into())
        })
    }

    /// Get events optionnally filtered by:
    /// * start slot
    /// * end slot
    /// * emitter address
    /// * original caller address
    /// * operation id
    pub async fn get_filtered_sc_output_event(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
        emitter_address: Option<Address>,
        original_caller_address: Option<Address>,
        original_operation_id: Option<OperationId>,
    ) -> Result<Vec<SCOutputEvent>, ExecutionError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(ExecutionCommand::GetSCOutputEvents {
                start,
                end,
                emitter_address,
                original_caller_address,
                original_operation_id,
                response_tx,
            })
            .await
            .map_err(|_| {
                ExecutionError::ChannelError("could not send GetSCOutputEvents command".into())
            })?;
        response_rx.await.map_err(|_| {
            ExecutionError::ChannelError("could not send GetSCOutputEvents upstream".into())
        })
    }

    /// Execute code in read-only mode.
    pub async fn execute_read_only_request(
        &self,
        max_gas: u64,
        simulated_gas_price: Amount,
        bytecode: Vec<u8>,
        address: Option<Address>,
    ) -> Result<ExecuteReadOnlyResponse, ExecutionError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(ExecutionCommand::ExecuteReadOnlyRequest {
                max_gas,
                simulated_gas_price,
                bytecode,
                result_sender: response_tx,
                address,
            })
            .await
            .map_err(|_| {
                ExecutionError::ChannelError("could not send ExecuteReadOnlyRequest command".into())
            })?;
        response_rx.await.map_err(|_| {
            ExecutionError::ChannelError("could not send ExecuteReadOnlyResponse upstream".into())
        })
    }

    pub async fn get_sce_ledger_for_addresses(
        self,
        addresses: Vec<Address>,
    ) -> Result<Map<Address, SCELedgerInfo>, ExecutionError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(ExecutionCommand::GetSCELedgerForAddresses {
                response_tx,
                addresses,
            })
            .await
            .map_err(|_| {
                ExecutionError::ChannelError(
                    "could not send GetSCELedgerForAddresses command".into(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ExecutionError::ChannelError("could not send GetSCELedgerForAddresses upstream".into())
        })
    }
}
