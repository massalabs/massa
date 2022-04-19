// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    commands::NetworkManagementCommand, error::NetworkError, BootstrapPeers, NetworkCommand,
    NetworkEvent, Peers,
};
use massa_models::{
    composite::PubkeySig,
    node::NodeId,
    operation::{OperationIds, Operations},
    stats::NetworkStats,
    BlockId, SignedEndorsement,
};
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
};
use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

/// Network command sender
#[derive(Clone)]
pub struct NetworkCommandSender(pub mpsc::Sender<NetworkCommand>);

impl NetworkCommandSender {
    /// ban by node
    pub async fn ban(&self, node_id: NodeId) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::Ban(node_id))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send Ban command".into()))?;
        Ok(())
    }

    /// ban ip
    pub async fn ban_ip(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::BanIp(ips))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send BanIp command".into()))?;
        Ok(())
    }

    /// add ip to whitelist
    pub async fn whitelist(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
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

    /// remove from banned nodes
    pub async fn unban(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::Unban(ips))
            .await
            .map_err(|_| NetworkError::ChannelError("could not send Unban command".into()))?;
        Ok(())
    }

    /// Send the order to send block.
    pub async fn send_block(&self, node: NodeId, block_id: BlockId) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendBlock { node, block_id })
            .await
            .map_err(|_| NetworkError::ChannelError("could not send SendBlock command".into()))?;
        Ok(())
    }

    /// Send the order to ask for a block.
    pub async fn ask_for_block_list(
        &self,
        list: HashMap<NodeId, Vec<BlockId>>,
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
        block_id: BlockId,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendBlockHeader { node, block_id })
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

    /// get network stats
    pub async fn get_network_stats(&self) -> Result<NetworkStats, NetworkError> {
        let (response_tx, response_rx) = oneshot::channel();
        self.0
            .send(NetworkCommand::GetStats { response_tx })
            .await
            .map_err(|_| NetworkError::ChannelError("could not send GetPeers command".into()))?;
        response_rx.await.map_err(|_| {
            NetworkError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })
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

    /// send block not found to node
    pub async fn block_not_found(
        &self,
        node: NodeId,
        block_id: BlockId,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::BlockNotFound { node, block_id })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send block_not_found command".into())
            })?;
        Ok(())
    }

    /// send operations to node
    pub async fn send_operations(
        &self,
        node: NodeId,
        operations: Operations,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendOperations { node, operations })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send SendOperations command".into())
            })?;
        Ok(())
    }

    /// Create a new call to the network, sending a announcement of `OperationIds` to a
    /// target node (`to_node`)
    ///
    /// # Returns
    /// Can return a `[NetworkError::ChannelError]` that must be managed by the direct caller of the
    /// function.
    pub async fn send_operations_batch(
        &self,
        to_node: NodeId,
        batch: OperationIds,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendOperationAnnouncements { to_node, batch })
            .await
            .map_err(|_| {
                NetworkError::ChannelError(
                    "could not send SendOperationAnnouncements command".into(),
                )
            })?;
        Ok(())
    }

    /// Create a new call to the network, sending a `wishlist` of `operationIds` to a
    /// target node (`to_node`) in order to receive the full operations in the future.
    ///
    /// # Returns
    /// Can return a `[NetworkError::ChannelError]` that must be managed by the direct caller of the
    /// function.
    pub fn send_ask_for_operations(
        &self,
        to_node: NodeId,
        wishlist: OperationIds,
    ) -> Result<(), NetworkError> {
        match self.0.try_reserve() {
            Ok(permit) => {
                permit.send(NetworkCommand::AskForOperations { to_node, wishlist });
                Ok(())
            }
            Err(_) => Err(NetworkError::ChannelError(
                "Failed to acquire permit to send AskForOperations command".into(),
            )),
        }
    }

    /// send endorsements to node id
    pub async fn send_endorsements(
        &self,
        node: NodeId,
        endorsements: Vec<SignedEndorsement>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendEndorsements { node, endorsements })
            .await
            .map_err(|_| {
                NetworkError::ChannelError("could not send send_endorsement command".into())
            })?;
        Ok(())
    }

    /// Sign a message using the node's private key
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
}

/// network event receiver
pub struct NetworkEventReceiver(pub mpsc::Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    /// wait network event
    pub async fn wait_event(&mut self) -> Result<NetworkEvent, NetworkError> {
        let res = self
            .0
            .recv()
            .await
            .ok_or_else(|| NetworkError::ChannelError("could not receive event".into()));
        res
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
        drop(self.manager_tx);
        let _remaining_events = network_event_receiver.drain().await;
        let _ = self.join_handle.await?;
        Ok(())
    }
}
