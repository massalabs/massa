// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    commands::{AskForBlocksInfo, NetworkManagementCommand},
    error::NetworkError,
    BlockInfoReply, BootstrapPeers, NetworkCommand, NetworkEvent, Peers,
};
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    composite::PubkeySig,
    endorsement::SecureShareEndorsement,
    node::NodeId,
    operation::{OperationPrefixIds, SecureShareOperation},
    stats::NetworkStats,
};

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
};
use tokio::{
    sync::{
        mpsc::{self, error::TrySendError},
        oneshot,
    },
    task::JoinHandle,
};
use tracing::{info, warn};

/// Network command sender
#[derive(Debug, Clone)]
pub struct NetworkCommandSender(pub mpsc::Sender<NetworkCommand>);

#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
impl NetworkCommandSender {
    /// ban node(s) by id(s)
    pub async fn node_ban_by_ids(&self, ids: Vec<NodeId>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeBanByIds(ids))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send BanId command".into()))?;
        Ok(())
    }

    /// ban node(s) by ip(s)
    pub async fn node_ban_by_ips(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeBanByIps(ips))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send BanIp command".into()))?;
        Ok(())
    }

    /// add ip to whitelist
    pub async fn add_to_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::Whitelist(ips))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send Whitelist command".into()))?;
        Ok(())
    }

    /// remove ip from whitelist
    pub async fn remove_from_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::RemoveFromWhitelist(ips))
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send RemoveFromWhitelist command".into())
            })?;
        Ok(())
    }

    /// remove from banned node(s) by id(s)
    pub async fn node_unban_by_ids(&self, ids: Vec<NodeId>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeUnbanByIds(ids))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send Unban command".into()))?;
        Ok(())
    }

    /// remove from banned node(s) by ip(s)
    pub async fn node_unban_ips(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeUnbanByIps(ips))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send Unban command".into()))?;
        Ok(())
    }

    /// Send info about the contents of a block.
    pub async fn send_block_info(
        &self,
        node: NodeId,
        info: Vec<(BlockId, BlockInfoReply)>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendBlockInfo { node, info })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send SendBlockInfo command".into())
            })?;
        Ok(())
    }

    /// Send the order to ask for a block.
    pub async fn ask_for_block_list(
        &self,
        list: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::AskForBlocks { list })
            .await
            .map_err(|_| NetworkError::ChannelError("could not send AskForBlock command".into()))?;
        Ok(())
    }

    /// Send the order to send block header.
    ///
    /// Note: with the current use of shared storage,
    /// sending a header requires having the block stored.
    /// This matches the current use of `send_block_header`,
    /// which is only used after a block has been integrated in the graph.
    pub async fn send_block_header(
        &self,
        node: NodeId,
        header: SecuredHeader,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendBlockHeader { node, header })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send SendBlockHeader command".into())
            })?;
        Ok(())
    }

    /// Send the order to get peers.
    pub async fn get_peers(&self) -> Result<Peers, NetworkError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(NetworkCommand::GetPeers(response_tx))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send GetPeers command".into()))?;
        response_rx.await.map_err(|_| {
            NetworkError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })
    }

    #[allow(missing_docs)]
    pub async fn get_network_stats(&self) -> Result<NetworkStats, NetworkError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(NetworkCommand::GetStats { response_tx })
            .await
            .map_err(|_| NetworkError::ChannelError("could not send GetStats command".into()))?;
        response_rx
            .await
            .map_err(|_| NetworkError::ChannelError("could not send GetStats upstream".into()))
    }

    /// Send the order to get bootstrap peers.
    pub async fn get_bootstrap_peers(&self) -> Result<BootstrapPeers, NetworkError> {
        let (response_tx, response_rx) = oneshot::channel::<BootstrapPeers>();
        self.0
            .send(NetworkCommand::GetBootstrapPeers(response_tx))
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send GetBootstrapPeers command".into())
            })?;
        response_rx.await.map_err(|_| {
            NetworkError::ChannelError("could not send GetBootstrapPeers response upstream".into())
        })
    }

    /// send operations to node
    pub async fn send_operations(
        &self,
        node: NodeId,
        operations: Vec<SecureShareOperation>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendOperations { node, operations })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send SendOperations command".into())
            })?;
        Ok(())
    }

    /// Create a new call to the network, sending a announcement of operation ID prefixes to a
    /// target node (`to_node`)
    ///
    /// # Returns
    /// Can return a `[NetworkError::ChannelError]` that must be managed by the direct caller of the
    /// function.
    pub async fn announce_operations(
        &self,
        to_node: NodeId,
        batch: OperationPrefixIds,
    ) -> Result<(), NetworkError> {
        match self
            .0
            .try_send(NetworkCommand::SendOperationAnnouncements { to_node, batch })
        {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                warn!("Failed to send NetworkCommand SendOperationAnnouncements channel full");
            }
            Err(TrySendError::Closed(_)) => {
                return Err(NetworkError::ChannelError(
                    "could not send SendOperationAnnouncements command".into(),
                ));
            }
        };
        Ok(())
    }

    /// Create a new call to the network, sending a `wishlist` of `operationIds` to a
    /// target node (`to_node`) in order to receive the full operations in the future.
    ///
    /// # Returns
    /// Can return a `[NetworkError::ChannelError]` that must be managed by the direct caller of the
    /// function.
    pub async fn send_ask_for_operations(
        &self,
        to_node: NodeId,
        wishlist: OperationPrefixIds,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::AskForOperations { to_node, wishlist })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send AskForOperations command".into())
            })?;
        Ok(())
    }

    /// send endorsements to node id
    pub async fn send_endorsements(
        &self,
        node: NodeId,
        endorsements: Vec<SecureShareEndorsement>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendEndorsements { node, endorsements })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send send_endorsement command".into())
            })?;
        Ok(())
    }

    /// Sign a message using the node's keypair
    pub async fn node_sign_message(&self, msg: Vec<u8>) -> Result<PubkeySig, NetworkError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(NetworkCommand::NodeSignMessage { msg, response_tx })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send GetBootstrapPeers command".into())
            })?;
        response_rx.await.map_err(|_| {
            NetworkError::ChannelError("could not send GetBootstrapPeers response upstream".into())
        })
    }

    #[cfg(any(test, feature = "testing"))]
    /// Used for mock-testing. Easier than using a clone derive
    #[allow(clippy::should_implement_trait)]
    pub fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// network event receiver
pub struct NetworkEventReceiver(pub mpsc::Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    /// wait network event
    pub async fn wait_event(&mut self) -> Result<NetworkEvent, NetworkError> {
        self.0
            .recv()
            .await
            .ok_or_else(|| NetworkError::ChannelError("could not receive event".into()))
    }

    /// drains remaining events and returns them in a `VecDeque`
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<NetworkEvent> {
        let mut remaining_events: VecDeque<NetworkEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

/// Network manager
pub struct NetworkManager {
    /// network handle
    pub join_handle: JoinHandle<Result<(), NetworkError>>,
    /// management commands
    pub manager_tx: mpsc::Sender<NetworkManagementCommand>,
}

impl NetworkManager {
    /// stop network
    pub async fn stop(
        self,
        network_event_receiver: NetworkEventReceiver,
    ) -> Result<(), NetworkError> {
        info!("stopping network manager...");
        drop(self.manager_tx);
        let _remaining_events = network_event_receiver.drain().await;
        let _ = self.join_handle.await?;
        info!("network manager stopped");
        Ok(())
    }
}

/// Used by the bootstrap server to run async tasks, allowing the bootstrap module to
/// remove the tokio dependency.
pub fn make_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("network-provided-runtime")
        .build()
        .expect("failed to create runtime")
}
