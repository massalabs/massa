use super::{
    common::NodeId,
    config::{ProtocolConfig, CHANNEL_SIZE},
    protocol_worker::{ProtocolCommand, ProtocolEvent, ProtocolManagementCommand, ProtocolWorker},
};
use crate::{
    error::CommunicationError,
    network::{NetworkCommandSender, NetworkEventReceiver},
};
use crypto::{hash::Hash, signature::SignatureEngine};
use models::block::Block;
use std::collections::VecDeque;
use storage::storage_controller::StorageCommandSender;
use tokio::{sync::mpsc, task::JoinHandle};

/// start a new ProtocolController from a ProtocolConfig
/// - generate public / private key
/// - create protocol_command/protocol_event channels
/// - lauch protocol_controller_fn in an other task
///
/// # Arguments
/// * cfg : protocol configuration
/// * network_command_sender: the NetworkCommandSender we interact with
/// * network_event_receiver: the NetworkEventReceiver we interact with
pub async fn start_protocol_controller(
    cfg: ProtocolConfig,
    network_command_sender: NetworkCommandSender,
    network_event_receiver: NetworkEventReceiver,
    opt_storage_command_sender: Option<StorageCommandSender>,
) -> Result<
    (
        ProtocolCommandSender,
        ProtocolEventReceiver,
        ProtocolManager,
    ),
    CommunicationError,
> {
    debug!("starting protocol controller");

    // generate our own random PublicKey (and therefore NodeId) and keep private key
    let signature_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let self_node_id = NodeId(signature_engine.derive_public_key(&private_key));

    debug!("local protocol node_id={:?}", self_node_id);
    massa_trace!("self_node_id", { "node_id": self_node_id });

    // launch worker
    let (event_tx, event_rx) = mpsc::channel::<ProtocolEvent>(CHANNEL_SIZE);
    let (command_tx, command_rx) = mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ProtocolManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        ProtocolWorker::new(
            cfg,
            self_node_id,
            private_key,
            network_command_sender,
            network_event_receiver,
            opt_storage_command_sender,
            event_tx,
            command_rx,
            manager_rx,
        )
        .run_loop()
        .await
    });
    debug!("protocol controller ready");
    Ok((
        ProtocolCommandSender(command_tx),
        ProtocolEventReceiver(event_rx),
        ProtocolManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct ProtocolCommandSender(pub mpsc::Sender<ProtocolCommand>);

impl ProtocolCommandSender {
    /// Sends the order to propagate that block
    ///
    /// # Arguments
    /// * hash : hash of the block header
    /// * block : block we want to propagate
    pub async fn propagate_block(
        &mut self,
        hash: Hash,
        block: &Block,
    ) -> Result<(), CommunicationError> {
        massa_trace!("block_propagation_order", { "block": hash });
        self.0
            .send(ProtocolCommand::PropagateBlock {
                hash,
                block: block.clone(),
            })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("propagate_block command send error".into())
            })
    }
}

pub struct ProtocolEventReceiver(pub mpsc::Receiver<ProtocolEvent>);

impl ProtocolEventReceiver {
    /// Receives the next ProtocolEvent from connected Node.
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> Result<ProtocolEvent, CommunicationError> {
        self.0.recv().await.ok_or(CommunicationError::ChannelError(
            "DefaultProtocolController wait_event channel recv failed".into(),
        ))
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ProtocolEvent> {
        let mut remaining_events: VecDeque<ProtocolEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

pub struct ProtocolManager {
    join_handle: JoinHandle<Result<NetworkEventReceiver, CommunicationError>>,
    manager_tx: mpsc::Sender<ProtocolManagementCommand>,
}

impl ProtocolManager {
    /// Stop the protocol controller
    pub async fn stop(
        self,
        protocol_event_receiver: ProtocolEventReceiver,
    ) -> Result<NetworkEventReceiver, CommunicationError> {
        drop(self.manager_tx);
        let _remaining_events = protocol_event_receiver.drain().await;
        let network_event_receiver = self.join_handle.await??;
        Ok(network_event_receiver)
    }
}
