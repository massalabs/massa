use super::{
    block_graph::*,
    config::{ConsensusConfig, CHANNEL_SIZE},
    consensus_worker::{
        ConsensusCommand, ConsensusEvent, ConsensusManagementCommand, ConsensusWorker,
    },
    error::ConsensusError,
};
use communication::protocol::{ProtocolCommandSender, ProtocolEventReceiver};
use crypto::signature::PublicKey;
use logging::debug;
use models::{Block, SerializationContext, Slot};
use std::collections::VecDeque;
use storage::StorageAccess;
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Creates a new consensus controller.
///
/// # Arguments
/// * cfg: consensus configuration
/// * protocol_command_sender: a ProtocolCommandSender instance to send commands to Protocol.
/// * protocol_event_receiver: a ProtocolEventReceiver instance to receive events from Protocol.
pub async fn start_consensus_controller(
    cfg: ConsensusConfig,
    serialization_context: SerializationContext,
    protocol_command_sender: ProtocolCommandSender,
    protocol_event_receiver: ProtocolEventReceiver,
    opt_storage_command_sender: Option<StorageAccess>,
) -> Result<
    (
        ConsensusCommandSender,
        ConsensusEventReceiver,
        ConsensusManager,
    ),
    ConsensusError,
> {
    debug!("starting consensus controller");
    massa_trace!("start", {});

    // ensure that the parameters are sane
    if cfg.thread_count == 0 {
        return Err(ConsensusError::ConfigError(format!(
            "thread_count shoud be strictly more than 0"
        )));
    }
    if cfg.t0 == 0.into() {
        return Err(ConsensusError::ConfigError(format!(
            "t0 shoud be strictly more than 0"
        )));
    }
    if cfg.t0.checked_rem_u64(cfg.thread_count as u64)? != 0.into() {
        return Err(ConsensusError::ConfigError(format!(
            "thread_count should divide t0"
        )));
    }

    // start worker
    let block_db = BlockGraph::new(cfg.clone(), serialization_context.clone())?;
    let (command_tx, command_rx) = mpsc::channel::<ConsensusCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<ConsensusEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ConsensusManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        ConsensusWorker::new(
            cfg_copy,
            protocol_command_sender,
            protocol_event_receiver,
            opt_storage_command_sender,
            block_db,
            command_rx,
            event_tx,
            manager_rx,
        )?
        .run_loop()
        .await
    });
    Ok((
        ConsensusCommandSender(command_tx),
        ConsensusEventReceiver(event_rx),
        ConsensusManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct ConsensusCommandSender(pub mpsc::Sender<ConsensusCommand>);

impl ConsensusCommandSender {
    /// Gets all the aviable information on the block graph returning a Blockgraphexport.
    pub async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<BlockGraphExport>();
        self.0
            .send(ConsensusCommand::GetBlockGraphStatus(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        })
    }

    /// Gets the whole block corresponding to given hash.
    ///
    /// # Arguments
    /// * hash: hash corresponding to the block we want.
    pub async fn get_active_block(
        &self,
        hash: crypto::hash::Hash,
    ) -> Result<Option<Block>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Option<Block>>();
        self.0
            .send(ConsensusCommand::GetActiveBlock(hash, response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        })
    }

    /// Gets (slot, public_key) were the staker with public_key was selected for slot, between start_slot and end_slot.
    ///
    /// # Arguments
    /// * start_slot: begining of the considered interval.
    /// * end_slot: end of the considered interval.
    pub async fn get_selection_draws(
        &self,
        start_slot: Slot,
        end_slot: Slot,
    ) -> Result<Vec<(Slot, PublicKey)>, ConsensusError> {
        let (response_tx, response_rx) =
            oneshot::channel::<Result<Vec<(Slot, PublicKey)>, ConsensusError>>();
        self.0
            .send(ConsensusCommand::GetSelectionDraws(
                start_slot,
                end_slot,
                response_tx,
            ))
            .await
            .map_err(|_| ConsensusError::SendChannelError("send error consensus command".into()))?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        })?
    }
}

pub struct ConsensusEventReceiver(pub mpsc::Receiver<ConsensusEvent>);

impl ConsensusEventReceiver {
    pub async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        self.0
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError)
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ConsensusEvent> {
        let mut remaining_events: VecDeque<ConsensusEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

pub struct ConsensusManager {
    join_handle: JoinHandle<Result<ProtocolEventReceiver, ConsensusError>>,
    manager_tx: mpsc::Sender<ConsensusManagementCommand>,
}

impl ConsensusManager {
    pub async fn stop(
        self,
        consensus_event_receiver: ConsensusEventReceiver,
    ) -> Result<ProtocolEventReceiver, ConsensusError> {
        drop(self.manager_tx);
        let _remaining_events = consensus_event_receiver.drain().await;
        let protocol_event_receiver = self.join_handle.await??;
        Ok(protocol_event_receiver)
    }
}
