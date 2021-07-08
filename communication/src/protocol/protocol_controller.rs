use super::{
    config::{ProtocolConfig, CHANNEL_SIZE},
    protocol_worker::{ProtocolCommand, ProtocolEvent, ProtocolManagementCommand, ProtocolWorker},
};
use crate::{
    error::CommunicationError,
    network::{NetworkCommandSender, NetworkEventReceiver},
};
use crypto::hash::Hash;
use models::{Block, BlockHeader, SerializationContext};
use std::collections::{HashSet, VecDeque};
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
    serialization_context: SerializationContext,
    network_command_sender: NetworkCommandSender,
    network_event_receiver: NetworkEventReceiver,
) -> Result<
    (
        ProtocolCommandSender,
        ProtocolEventReceiver,
        ProtocolManager,
    ),
    CommunicationError,
> {
    debug!("starting protocol controller");

    // launch worker
    let (event_tx, event_rx) = mpsc::channel::<ProtocolEvent>(CHANNEL_SIZE);
    let (command_tx, command_rx) = mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ProtocolManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        ProtocolWorker::new(
            cfg,
            serialization_context,
            network_command_sender,
            network_event_receiver,
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
    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * hash : hash of the block header
    pub async fn propagate_block_header(
        &mut self,
        hash: Hash,
        header: BlockHeader,
    ) -> Result<(), CommunicationError> {
        massa_trace!("block_header_propagation_order", { "block": hash });
        self.0
            .send(ProtocolCommand::PropagateBlockHeader { hash, header })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("propagate_block_header command send error".into())
            })
    }

    /// Sends a block to a peer.
    ///
    /// # Arguments
    /// * block : the block.
    /// * node: the id of the node to send the block to.
    pub async fn send_block(&mut self, hash: Hash, block: Block) -> Result<(), CommunicationError> {
        massa_trace!("send_block_order", { "block": block });
        self.0
            .send(ProtocolCommand::SendBlock { hash, block })
            .await
            .map_err(|_| CommunicationError::ChannelError("send_block command send error".into()))
    }

    pub async fn send_wishlist_delta(
        &mut self,
        new: HashSet<Hash>,
        remove: HashSet<Hash>,
    ) -> Result<(), CommunicationError> {
        massa_trace!("send_wishlist_delta", {});
        self.0
            .send(ProtocolCommand::WishlistDelta { new, remove })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("send_wishlist_delta command send error".into())
            })
    }

    pub async fn block_not_found(&mut self, hash: Hash) -> Result<(), CommunicationError> {
        massa_trace!("block_not_found", {});
        self.0
            .send(ProtocolCommand::BlockNotFound(hash))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("block_not_found command send error".into())
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
