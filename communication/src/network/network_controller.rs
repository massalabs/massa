// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::{
    config::{NetworkConfig, CHANNEL_SIZE},
    establisher::Establisher,
    network_worker::{
        NetworkCommand, NetworkEvent, NetworkManagementCommand, NetworkWorker, Peers,
    },
    peer_info_database::*,
    BootstrapPeers,
};
use crate::common::NodeId;
use crate::error::CommunicationError;
use crypto::signature::{
    derive_public_key, generate_random_private_key, PrivateKey, PublicKey, Signature,
};
use models::{Block, BlockHeader, BlockId, Endorsement, Operation, Version};
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
    mut establisher: Establisher,
    clock_compensation: i64,
    initial_peers: Option<BootstrapPeers>,
    version: Version,
) -> Result<
    (
        NetworkCommandSender,
        NetworkEventReceiver,
        NetworkManager,
        PrivateKey,
    ),
    CommunicationError,
> {
    debug!("starting network controller");

    // check that local IP is routable
    if let Some(self_ip) = cfg.routable_ip {
        if !self_ip.is_global() {
            return Err(CommunicationError::InvalidIpError(self_ip));
        }
    }

    // try to read node private key from file, otherwise generate it & write to file. Then derive nodeId
    let private_key = if std::path::Path::is_file(&cfg.private_key_file) {
        // file exists: try to load it
        let private_key_bs58_check = tokio::fs::read_to_string(&cfg.private_key_file)
            .await
            .map_err(|err| {
                std::io::Error::new(
                    err.kind(),
                    format!("could not load node private key file: {:?}", err),
                )
            })?;
        PrivateKey::from_bs58_check(private_key_bs58_check.trim()).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("node private key file corrupted: {:?}", err),
            )
        })?
    } else {
        // node file does not exist: generate the key and save it
        let priv_key = generate_random_private_key();
        if let Err(e) = tokio::fs::write(&cfg.private_key_file, &priv_key.to_bs58_check()).await {
            warn!("could not generate node private key file: {:?}", e);
        }
        priv_key
    };
    let public_key = derive_public_key(&private_key);
    let self_node_id = NodeId(public_key);

    info!("The node_id of this node is: {:?}", self_node_id);
    massa_trace!("self_node_id", { "node_id": self_node_id });

    // create listener
    let listener = establisher.get_listener(cfg.bind).await?;

    // load peer info database
    let mut peer_info_db = PeerInfoDatabase::new(&cfg, clock_compensation).await?;

    // add initial peers
    if let Some(peers) = initial_peers {
        peer_info_db.merge_candidate_peers(&peers.0)?;
    }

    // launch controller
    let (command_tx, command_rx) = mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<NetworkManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = NetworkWorker::new(
            cfg_copy,
            private_key,
            self_node_id,
            listener,
            establisher,
            peer_info_db,
            command_rx,
            event_tx,
            manager_rx,
            version,
        )
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("network worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("network worker finished cleanly");
                Ok(v)
            }
        }
    });

    debug!("network controller started");

    Ok((
        NetworkCommandSender(command_tx),
        NetworkEventReceiver(event_rx),
        NetworkManager {
            join_handle,
            manager_tx,
        },
        private_key,
    ))
}

#[derive(Clone)]
pub struct NetworkCommandSender(pub mpsc::Sender<NetworkCommand>);

impl NetworkCommandSender {
    pub async fn ban(&self, node_id: NodeId) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::Ban(node_id))
            .await
            .map_err(|_| CommunicationError::ChannelError("could not send Ban command".into()))?;
        Ok(())
    }

    pub async fn unban(&self, ip: IpAddr) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::Unban(ip))
            .await
            .map_err(|_| CommunicationError::ChannelError("could not send Unban command".into()))?;
        Ok(())
    }

    /// Send the order to send block.
    pub async fn send_block(&self, node: NodeId, block: Block) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::SendBlock { node, block })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send SendBlock command".into())
            })?;
        Ok(())
    }

    /// Send the order to ask for a block.
    pub async fn ask_for_block_list(
        &self,
        list: HashMap<NodeId, Vec<BlockId>>,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::AskForBlocks { list })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send AskForBlock command".into())
            })?;
        Ok(())
    }

    /// Send the order to send block header.
    pub async fn send_block_header(
        &self,
        node: NodeId,
        header: BlockHeader,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::SendBlockHeader { node, header })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send SendBlockHeader command".into())
            })?;
        Ok(())
    }

    /// Send the order to get peers.
    pub async fn get_peers(&self) -> Result<Peers, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(NetworkCommand::GetPeers(response_tx))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send GetPeers command".into())
            })?;
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })?)
    }

    /// Send the order to get bootstrap peers.
    pub async fn get_bootstrap_peers(&self) -> Result<BootstrapPeers, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<BootstrapPeers>();
        self.0
            .send(NetworkCommand::GetBootstrapPeers(response_tx))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send GetBootstrapPeers command".into())
            })?;
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetBootstrapPeers response upstream".into(),
            )
        })?)
    }

    pub async fn block_not_found(
        &self,
        node: NodeId,
        block_id: BlockId,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::BlockNotFound { node, block_id })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send block_not_found command".into())
            })?;
        Ok(())
    }

    pub async fn send_operations(
        &self,
        node: NodeId,
        operations: Vec<Operation>,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::SendOperations { node, operations })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send SendOperations command".into())
            })?;
        Ok(())
    }

    pub async fn send_endorsements(
        &self,
        node: NodeId,
        endorsements: Vec<Endorsement>,
    ) -> Result<(), CommunicationError> {
        self.0
            .send(NetworkCommand::SendEndorsements { node, endorsements })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send send_endorsement command".into())
            })?;
        Ok(())
    }

    /// Sign a message using the node's private key
    pub async fn node_sign_message(
        &self,
        msg: Vec<u8>,
    ) -> Result<(PublicKey, Signature), CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<(PublicKey, Signature)>();
        self.0
            .send(NetworkCommand::NodeSignMessage { msg, response_tx })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("could not send GetBootstrapPeers command".into())
            })?;
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetBootstrapPeers response upstream".into(),
            )
        })?)
    }
}

pub struct NetworkEventReceiver(pub mpsc::Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    pub async fn wait_event(&mut self) -> Result<NetworkEvent, CommunicationError> {
        let res = self
            .0
            .recv()
            .await
            .ok_or_else(|| CommunicationError::ChannelError("could not receive event".into()));
        res
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
