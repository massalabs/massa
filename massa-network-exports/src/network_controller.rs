// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{
    commands::{AskForBlocksInfo, NetworkManagementCommand},
    error::NetworkError,
    BlockInfoReply, BootstrapPeers, NetworkCommand, NetworkEvent, Peers,
};
use crossbeam_channel::{bounded, Receiver, Sender};
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    composite::PubkeySig,
    endorsement::SecureShareEndorsement,
    node::NodeId,
    operation::{OperationPrefixIds, SecureShareOperation},
    stats::NetworkStats,
};
use std::thread::JoinHandle;
use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
};
use tracing::info;

/// Network command sender
#[derive(Clone)]
pub struct NetworkCommandSender(pub Sender<NetworkCommand>);

// TODO: refactor
impl NetworkCommandSender {
    /// ban node(s) by id(s)
    pub fn node_ban_by_ids(&self, ids: Vec<NodeId>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeBanByIds(ids))
            .map_err(|_| NetworkError::ChannelError("could not send BanId command".into()))?;
        Ok(())
    }

    /// ban node(s) by ip(s)
    pub fn node_ban_by_ips(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeBanByIps(ips))
            .map_err(|_| NetworkError::ChannelError("could not send BanIp command".into()))?;
        Ok(())
    }

    /// add ip to whitelist
    pub fn add_to_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::Whitelist(ips))
            .map_err(|_| NetworkError::ChannelError("could not send Whitelist command".into()))?;
        Ok(())
    }

    /// remove ip from whitelist
    pub fn remove_from_whitelist(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::RemoveFromWhitelist(ips))
            .map_err(|_| {
                NetworkError::ChannelError("could not send RemoveFromWhitelist command".into())
            })?;
        Ok(())
    }

    /// remove from banned node(s) by id(s)
    pub fn node_unban_by_ids(&self, ids: Vec<NodeId>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeUnbanByIds(ids))
            .map_err(|_| NetworkError::ChannelError("could not send Unban command".into()))?;
        Ok(())
    }

    /// remove from banned node(s) by ip(s)
    pub fn node_unban_ips(&self, ips: Vec<IpAddr>) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::NodeUnbanByIps(ips))
            .map_err(|_| NetworkError::ChannelError("could not send Unban command".into()))?;
        Ok(())
    }

    /// Send info about the contents of a block.
    pub fn send_block_info(
        &self,
        node: NodeId,
        info: Vec<(BlockId, BlockInfoReply)>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendBlockInfo { node, info })
            .map_err(|_| {
                NetworkError::ChannelError("could not send SendBlockInfo command".into())
            })?;
        Ok(())
    }

    /// Send the order to ask for a block.
    pub fn ask_for_block_list(
        &self,
        list: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::AskForBlocks { list })
            .map_err(|_| NetworkError::ChannelError("could not send AskForBlock command".into()))?;
        Ok(())
    }

    /// Send the order to send block header.
    ///
    /// Note: with the current use of shared storage,
    /// sending a header requires having the block stored.
    /// This matches the current use of `send_block_header`,
    /// which is only used after a block has been integrated in the graph.
    pub fn send_block_header(
        &self,
        node: NodeId,
        header: SecuredHeader,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendBlockHeader { node, header })
            .map_err(|_| {
                NetworkError::ChannelError("could not send SendBlockHeader command".into())
            })?;
        Ok(())
    }

    /// Send the order to get peers.
    pub fn get_peers(&self) -> Result<Peers, NetworkError> {
        let (response_tx, response_rx) = bounded(1);
        self.0
            .send(NetworkCommand::GetPeers(response_tx))
            .map_err(|_| NetworkError::ChannelError("could not send GetPeers command".into()))?;
        response_rx.recv().map_err(|_| {
            NetworkError::ChannelError(
                "could not send GetAdvertisablePeerListChannelError upstream".into(),
            )
        })
    }

    /// get network stats
    pub fn get_network_stats(&self) -> Result<NetworkStats, NetworkError> {
        let (response_tx, response_rx) = bounded(1);
        self.0
            .send(NetworkCommand::GetStats { response_tx })
            .map_err(|_| NetworkError::ChannelError("could not send GetStats command".into()))?;
        response_rx
            .recv()
            .map_err(|_| NetworkError::ChannelError("could not send GetStats upstream".into()))
    }

    /// Send the order to get bootstrap peers.
    pub fn get_bootstrap_peers(&self) -> Result<BootstrapPeers, NetworkError> {
        let (response_tx, response_rx) = bounded::<BootstrapPeers>(1);
        self.0
            .send(NetworkCommand::GetBootstrapPeers(response_tx))
            .map_err(|_| {
                NetworkError::ChannelError("could not send GetBootstrapPeers command".into())
            })?;
        response_rx.recv().map_err(|_| {
            NetworkError::ChannelError("could not send GetBootstrapPeers response upstream".into())
        })
    }

    /// send operations to node
    pub fn send_operations(
        &self,
        node: NodeId,
        operations: Vec<SecureShareOperation>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendOperations { node, operations })
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
    pub fn announce_operations(
        &self,
        to_node: NodeId,
        batch: OperationPrefixIds,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendOperationAnnouncements { to_node, batch })
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
        wishlist: OperationPrefixIds,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::AskForOperations { to_node, wishlist })
            .map_err(|_| {
                NetworkError::ChannelError("could not send AskForOperations command".into())
            })?;
        Ok(())
    }

    /// send endorsements to node id
    pub fn send_endorsements(
        &self,
        node: NodeId,
        endorsements: Vec<SecureShareEndorsement>,
    ) -> Result<(), NetworkError> {
        self.0
            .send(NetworkCommand::SendEndorsements { node, endorsements })
            .map_err(|_| {
                NetworkError::ChannelError("could not send send_endorsement command".into())
            })?;
        Ok(())
    }

    /// Sign a message using the node's keypair
    pub fn node_sign_message(&self, msg: Vec<u8>) -> Result<PubkeySig, NetworkError> {
        let (response_tx, response_rx) = bounded(1);
        self.0
            .send(NetworkCommand::NodeSignMessage { msg, response_tx })
            .map_err(|_| {
                NetworkError::ChannelError("could not send GetBootstrapPeers command".into())
            })?;
        response_rx.recv().map_err(|_| {
            NetworkError::ChannelError("could not send GetBootstrapPeers response upstream".into())
        })
    }
}

/// network event receiver
pub struct NetworkEventReceiver(pub Receiver<NetworkEvent>);

impl NetworkEventReceiver {
    /// wait network event
    pub fn wait_event(&mut self) -> Result<NetworkEvent, NetworkError> {
        self.0
            .recv()
            .map_err(|_| NetworkError::ChannelError("could not receive event".into()))
    }

    /// drains remaining events and returns them in a `VecDeque`
    /// note: events are sorted from oldest to newest
    pub fn drain(self) -> VecDeque<NetworkEvent> {
        let mut remaining_events: VecDeque<NetworkEvent> = VecDeque::new();
        while let Ok(evt) = self.0.recv() {
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
    pub manager_tx: Sender<NetworkManagementCommand>,
}

impl NetworkManager {
    /// stop network
    pub async fn stop(
        self,
        network_event_receiver: NetworkEventReceiver,
    ) -> Result<(), NetworkError> {
        info!("stopping network manager...");
        drop(self.manager_tx);
        let _remaining_events = network_event_receiver.drain();
        let _ = self
            .join_handle
            .join()
            .expect("Failed to join on the network worker thread.");
        info!("network manager stopped");
        Ok(())
    }
}
