use super::{
    config::{NetworkConfig, CHANNEL_SIZE},
    establisher::Establisher,
    network_worker::{NetworkCommand, NetworkEvent, NetworkManagementCommand, NetworkWorker},
    peer_info_database::*,
};
use crate::common::NodeId;
use crate::error::CommunicationError;
use crypto::{hash::Hash, signature::SignatureEngine};
use models::{Block, BlockHeader, SerializationContext};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Starts a new NetworkWorker in a spawned task
///
/// # Arguments
/// * cfg : network configuration
pub async fn start_network_controller(
    cfg: NetworkConfig,
    serialization_context: SerializationContext,
    mut establisher: Establisher,
) -> Result<(NetworkCommandSender, NetworkEventReceiver, NetworkManager), CommunicationError> {
    debug!("starting network controller");

    // check that local IP is routable
    if let Some(self_ip) = cfg.routable_ip {
        if !self_ip.is_global() {
            return Err(CommunicationError::InvalidIpError(self_ip));
        }
    }

    // generate our own random PublicKey (and therefore NodeId) and keep private key
    let signature_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let self_node_id = NodeId(signature_engine.derive_public_key(&private_key));

    debug!("local network node_id={:?}", self_node_id);
    massa_trace!("self_node_id", { "node_id": self_node_id });

    // create listener
    let listener = establisher.get_listener(cfg.bind).await?;

    // load peer info database
    let peer_info_db = PeerInfoDatabase::new(&cfg).await?;

    // launch controller
    let (command_tx, command_rx) = mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<NetworkManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        NetworkWorker::new(
            cfg_copy,
            serialization_context,
            private_key,
            self_node_id,
            listener,
            establisher,
            peer_info_db,
            command_rx,
            event_tx,
            manager_rx,
        )
        .run_loop()
        .await
    });

    debug!("network controller started");

    Ok((
        NetworkCommandSender(command_tx),
        NetworkEventReceiver(event_rx),
        NetworkManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct NetworkCommandSender(pub mpsc::Sender<NetworkCommand>);

impl NetworkCommandSender {
    /// Send the order to send block.
    pub async fn send_block(
        &mut self,
        node_id: NodeId,
        block: Block,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::SendBlock(node_id, block))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send SendBlock command".into())
            })?;
        Ok(())
    }

    /// Send the order to ask for a block.
    pub async fn ask_for_block(
        &mut self,
        node_id: NodeId,
        hash: Hash,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::AskForBlock(node_id, hash))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send AskForBlock command".into())
            })?;
        Ok(())
    }

    /// Send the order to send block header.
    pub async fn send_block_header(
        &mut self,
        node: NodeId,
        header: BlockHeader,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::SendBlockHeader { node, header })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send SendBlockHeader command".into())
            })?;
        Ok(())
    }

    /// Send the order to get peers.
    pub async fn get_peers(&mut self) -> Result<HashMap<IpAddr, PeerInfo>, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<HashMap<IpAddr, PeerInfo>>();
        self.0
            .send(NetworkCommand::GetPeers(response_tx))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send GetPeers command".into())
            })?;
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })?)
    }
}

pub struct NetworkEventReceiver(pub mpsc::Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    pub async fn wait_event(&mut self) -> Result<NetworkEvent, CommunicationError> {
        self.0.recv().await.ok_or(CommunicationError::ChannelError(
            "could not receive event".into(),
        ))
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<NetworkEvent> {
        let mut remaining_events: VecDeque<NetworkEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

pub struct NetworkManager {
    join_handle: JoinHandle<Result<(), CommunicationError>>,
    manager_tx: mpsc::Sender<NetworkManagementCommand>,
}

impl NetworkManager {
    pub async fn stop(
        self,
        network_event_receiver: NetworkEventReceiver,
    ) -> Result<(), CommunicationError> {
        drop(self.manager_tx);
        let _remaining_events = network_event_receiver.drain().await;
        let _ = self.join_handle.await?;
        Ok(())
    }
}
