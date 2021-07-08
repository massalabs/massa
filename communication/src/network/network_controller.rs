use super::{
    config::{NetworkConfig, CHANNEL_SIZE},
    establisher::Establisher,
    network_worker::{NetworkCommand, NetworkEvent, NetworkManagementCommand, NetworkWorker},
    peer_info_database::*,
};
use crate::common::NodeId;
use crate::error::CommunicationError;
use crypto::{
    hash::Hash,
    signature::{PrivateKey, SignatureEngine},
};
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
    clock_compensation: i64,
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
    let signature_engine = SignatureEngine::new();
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
        PrivateKey::from_bs58_check(&private_key_bs58_check.trim()).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("node private key file corrupted: {:?}", err),
            )
        })?
    } else {
        // node file does not exist: generate the key and save it
        let priv_key = SignatureEngine::generate_random_private_key();
        if let Err(e) = tokio::fs::write(&cfg.private_key_file, &priv_key.to_bs58_check()).await {
            warn!("could not generate node private key file: {:?}", e);
        }
        priv_key
    };
    let self_node_id = NodeId(signature_engine.derive_public_key(&private_key));

    debug!("local network node_id={:?}", self_node_id);
    massa_trace!("self_node_id", { "node_id": self_node_id });

    // create listener
    let listener = establisher.get_listener(cfg.bind).await?;

    // load peer info database
    let peer_info_db = PeerInfoDatabase::new(&cfg, clock_compensation).await?;

    // launch controller
    let (command_tx, command_rx) = mpsc::channel::<NetworkCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<NetworkEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<NetworkManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        let res = NetworkWorker::new(
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
    pub async fn ban(&mut self, node_id: NodeId) -> Result<(), CommunicationError> {
        trace!("before sending NetworkCommand::Ban from NetworkCommandSender.0 in NetworkCommandSender ban");
        self.0
            .send(NetworkCommand::Ban(node_id))
            .await
            .map_err(|_| CommunicationError::ChannelError("cound not send Ban command".into()))?;
        trace!("after sending NetworkCommand::Ban from NetworkCommandSender.0 in NetworkCommandSender ban");
        Ok(())
    }

    /// Send the order to send block.
    pub async fn send_block(
        &mut self,
        node: NodeId,
        block: Block,
    ) -> Result<(), CommunicationError> {
        trace!("before sending NetworkCommand::SendBlock from NetworkCommandSender.0 in NetworkCommandSender send_block");
        self.0
            .send(NetworkCommand::SendBlock { node, block })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send SendBlock command".into())
            })?;
        trace!("after sending NetworkCommand::SendBlock from NetworkCommandSender.0 in NetworkCommandSender send_block");
        Ok(())
    }

    /// Send the order to ask for a block.
    pub async fn ask_for_block_list(
        &mut self,
        list: HashMap<NodeId, Vec<Hash>>,
    ) -> Result<(), CommunicationError> {
        trace!("before sending NetworkCommand::AskForBlock from NetworkCommandSender.0 in NetworkCommandSender ask_for_block");
        self.0
            .send(NetworkCommand::AskForBlocks { list })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send AskForBlock command".into())
            })?;
        trace!("after sending NetworkCommand::AskForBlock from NetworkCommandSender.0 in NetworkCommandSender ask_for_block");
        Ok(())
    }

    /// Send the order to send block header.
    pub async fn send_block_header(
        &mut self,
        node: NodeId,
        header: BlockHeader,
    ) -> Result<(), CommunicationError> {
        trace!("before sending NetworkCommand::SendBlockHeader from NetworkCommandSender.0 in NetworkCommandSender send_block_header");
        self.0
            .send(NetworkCommand::SendBlockHeader { node, header })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send SendBlockHeader command".into())
            })?;
        trace!("after sending NetworkCommand::SendBlockHeader from NetworkCommandSender.0 in NetworkCommandSender send_block_header");
        Ok(())
    }

    /// Send the order to get peers.
    pub async fn get_peers(&mut self) -> Result<HashMap<IpAddr, PeerInfo>, CommunicationError> {
        let (response_tx, response_rx) = oneshot::channel::<HashMap<IpAddr, PeerInfo>>();
        trace!("before sending NetworkCommand::GetPeers from NetworkCommandSender.0 in NetworkCommandSender get_peers");
        self.0
            .send(NetworkCommand::GetPeers(response_tx))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send GetPeers command".into())
            })?;
        trace!("after sending NetworkCommand::GetPeers from NetworkCommandSender.0 in NetworkCommandSender get_peers");
        Ok(response_rx.await.map_err(|_| {
            CommunicationError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })?)
    }

    pub async fn block_not_found(
        &mut self,
        node: NodeId,
        hash: Hash,
    ) -> Result<(), CommunicationError> {
        trace!("before sending NetworkCommand::BlockNotFound from NetworkCommandSender.0 in NetworkCommandSender block_not_found");
        self.0
            .send(NetworkCommand::BlockNotFound { node, hash })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("cound not send block_not_found command".into())
            })?;
        trace!("after sending NetworkCommand::BlockNotFound from NetworkCommandSender.0 in NetworkCommandSender block_not_found");
        Ok(())
    }
}

pub struct NetworkEventReceiver(pub mpsc::Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    pub async fn wait_event(&mut self) -> Result<NetworkEvent, CommunicationError> {
        trace!("before receiving NetworkEvent from NetworkEventReceiver.0 in network_controler wait_event");
        let res = self.0.recv().await.ok_or(CommunicationError::ChannelError(
            "could not receive event".into(),
        ));
        trace!("after receiving NetworkEvent from NetworkEventReceiver.0 in network_controler wait_event");
        res
    }

    /// drains remaining events and returns them in a VecDeque
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<NetworkEvent> {
        let mut remaining_events: VecDeque<NetworkEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            trace!("after receiving NetworkEvent from NetworkEventReceiver.0 in network_controler drain");
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
