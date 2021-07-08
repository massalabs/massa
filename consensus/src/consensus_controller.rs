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
    boot_graph: Option<BoostrapableGraph>,
    clock_compensation: i64,
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
    let block_db = BlockGraph::new(cfg.clone(), serialization_context.clone(), boot_graph)?;
    let (command_tx, command_rx) = mpsc::channel::<ConsensusCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<ConsensusEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ConsensusManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = ConsensusWorker::new(
            cfg_copy,
            protocol_command_sender,
            protocol_event_receiver,
            opt_storage_command_sender,
            block_db,
            command_rx,
            event_tx,
            manager_rx,
            clock_compensation,
        )?
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("consensus worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("consensus worker finished cleanly");
                Ok(v)
            }
        }
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
        trace!("before sending get_block_graph_status command to consensus command sender in get_block_graph_status in consensus_controller");
        self.0
            .send(ConsensusCommand::GetBlockGraphStatus(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        trace!("after sending get_block_graph_status command to consensus command sender in get_block_graph_status in consensus_controller");
        trace!("before receiving block_graph export from oneshot response_rx in get_block_graph_status in consensus_controller");
        let res = response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        });
        trace!("after receiving block_graph export from oneshot response_rx in get_block_graph_status in consensus_controller");
        res
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
        trace!("before sending get_active_block command to consensus command sender in get_active_block in consensus_controller");
        self.0
            .send(ConsensusCommand::GetActiveBlock { hash, response_tx })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(format!("send error consensus command"))
            })?;
        trace!("after sending get_active_block command to consensus command sender in get_active_block in consensus_controller");
        trace!("before receiving block from oneshot response_rx in get_active_block in consensus_controller");
        let res = response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        });
        trace!("after receiving block from oneshot response_rx in get_active_block in consensus_controller");
        res
    }

    /// Gets (slot, public_key) were the staker with public_key was selected for slot, between start_slot and end_slot.
    ///
    /// # Arguments
    /// * start_slot: begining of the considered interval.
    /// * end_slot: end of the considered interval.
    pub async fn get_selection_draws(
        &self,
        start: Slot,
        end: Slot,
    ) -> Result<Vec<(Slot, PublicKey)>, ConsensusError> {
        let (response_tx, response_rx) =
            oneshot::channel::<Result<Vec<(Slot, PublicKey)>, ConsensusError>>();
        trace!("before sending get_selection_draws command to consensus command sender in get_selection_draws in consensus_controller");
        self.0
            .send(ConsensusCommand::GetSelectionDraws {
                start,
                end,
                response_tx,
            })
            .await
            .map_err(|_| ConsensusError::SendChannelError("send error consensus command".into()))?;
        trace!("after sending get_selection_draws command to consensus command sender in get_selection_draws in consensus_controller");
        trace!("before receiving slots and pub keys from oneshot response_rx in get_selection_draws in consensus_controller");
        let res = response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        })?;
        trace!("after receiving slots and pub keys from oneshot response_rx in get_selection_draws in consensus_controller");
        res
    }

    pub async fn get_bootstrap_graph(&self) -> Result<BoostrapableGraph, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<BoostrapableGraph>();
        trace!("before sending get_bootstrap_graph command to consensus command sender in get_bootstrap_graph in consensus_controller");
        self.0
            .send(ConsensusCommand::GetBootGraph(response_tx))
            .await
            .map_err(|_| ConsensusError::SendChannelError("send error consensus command".into()))?;

        trace!("after sending get_bootstrap_graph command to consensus command sender in get_bootstrap_graph in consensus_controller");
        trace!("before receiving boot graph from oneshot response_rx in get_bootstrap_graph in consensus_controller");
        let res = response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(format!("consensus command response read error"))
        });

        trace!("after receiving boot graph from oneshot response_rx in get_bootstrap_graph in consensus_controller");
        res
    }
}

pub struct ConsensusEventReceiver(pub mpsc::Receiver<ConsensusEvent>);

impl ConsensusEventReceiver {
    pub async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        trace!("before receiving next event from consensus event receiver in wait_event in consensus_controller");
        let evt = self
            .0
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError);
        trace!("after receiving next event from consensus event receiver in wait_event in consensus_controller");
        evt
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ConsensusEvent> {
        let mut remaining_events: VecDeque<ConsensusEvent> = VecDeque::new();

        trace!("before receiving next event from consensus event receiver in drain in consensus_controller");
        while let Some(evt) = self.0.recv().await {
            trace!("after receiving next event from consensus event receiver in drain in consensus_controller");
            remaining_events.push_back(evt);
            // should i add a before receiving next event here ?
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
