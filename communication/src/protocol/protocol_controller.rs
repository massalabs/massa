use super::{
    config::{ProtocolConfig, CHANNEL_SIZE},
    protocol_worker::{ProtocolCommand, ProtocolEvent, ProtocolManagementCommand, ProtocolWorker},
};
use crate::{
    error::CommunicationError,
    network::{NetworkCommandSender, NetworkEventReceiver},
};
use crypto::hash::Hash;
use hang_monitor::HangMonitorCommandSender;
use models::{Block, SerializationContext};
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
    hang_monitor: Option<HangMonitorCommandSender>,
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
        let res = ProtocolWorker::new(
            cfg,
            serialization_context,
            network_command_sender,
            network_event_receiver,
            event_tx,
            command_rx,
            manager_rx,
            hang_monitor,
        )
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("protocol worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("protocol worker finished cleanly");
                Ok(v)
            }
        }
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
    pub async fn integrated_block(
        &mut self,
        hash: Hash,
        block: Block,
    ) -> Result<(), CommunicationError> {
        massa_trace!("block_integrated_order", { "block": hash });
        self.0
            .send(ProtocolCommand::IntegratedBlock { hash, block })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("block_integrated command send error".into())
            })
    }

    /// Notify to protocol an attack attempt.
    pub async fn notify_block_attack(&mut self, hash: Hash) -> Result<(), CommunicationError> {
        massa_trace!("notify_block_attack_order", { "object": hash });
        self.0
            .send(ProtocolCommand::AttackBlockDetected(hash))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("notify_block_attack command send error".into())
            })
    }

    /// Sends a block to a peer.
    ///
    /// # Arguments
    /// * block : the block.
    /// * node: the id of the node to send the block to.
    pub async fn found_block(
        &mut self,
        hash: Hash,
        block: Block,
    ) -> Result<(), CommunicationError> {
        massa_trace!("found_block_order", { "block": block });
        self.0
            .send(ProtocolCommand::FoundBlock { hash, block })
            .await
            .map_err(|_| CommunicationError::ChannelError("found_block command send error".into()))
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
