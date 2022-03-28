use std::{collections::HashMap, net::IpAddr};

use crate::{BootstrapPeers, Peers};
use massa_models::SignedEndorsement;
use massa_models::SignedHeader;
use massa_models::SignedOperation;
use massa_models::{composite::PubkeySig, node::NodeId, stats::NetworkStats, Block, BlockId};
use tokio::sync::oneshot;

/// Commands that the worker can execute
#[derive(Debug)]
pub enum NetworkCommand {
    /// Ask for a block from a node.
    AskForBlocks {
        /// node to block ids
        list: HashMap<NodeId, Vec<BlockId>>,
    },
    /// Send that block to node.
    SendBlock {
        /// to node id
        node: NodeId,
        /// block
        block: Block,
    },
    /// Send a header to a node.
    SendBlockHeader {
        /// to node id
        node: NodeId,
        /// header
        header: SignedHeader,
    },
    /// (PeerInfo, Vec <(NodeId, bool)>) peer info + list of associated Id nodes in connection out (true)
    GetPeers(oneshot::Sender<Peers>),
    /// get peers for bootstrap server
    GetBootstrapPeers(oneshot::Sender<BootstrapPeers>),
    /// ban by node id
    Ban(NodeId),
    /// ban by ips
    BanIp(Vec<IpAddr>),
    /// unban ips
    Unban(Vec<IpAddr>),
    /// send block not found notification to node id
    BlockNotFound {
        /// to node id
        node: NodeId,
        /// block id
        block_id: BlockId,
    },
    /// send operations to node id
    SendOperations {
        /// to node id
        node: NodeId,
        /// operations
        operations: Vec<SignedOperation>,
    },
    /// send endorsements to node id
    SendEndorsements {
        /// to node id
        node: NodeId,
        /// endorsements
        endorsements: Vec<SignedEndorsement>,
    },
    /// sign message with our node private key (associated to node id)
    /// != staking key
    NodeSignMessage {
        /// arbitrary message
        msg: Vec<u8>,
        /// response channels
        response_tx: oneshot::Sender<PubkeySig>,
    },
    /// gets network stats
    GetStats {
        /// response channels
        response_tx: oneshot::Sender<NetworkStats>,
    },
    /// add ip to whitelist
    Whitelist(Vec<IpAddr>),
    /// remove ips from whitelist
    RemoveFromWhitelist(Vec<IpAddr>),
}

/// network event
#[derive(Debug)]
pub enum NetworkEvent {
    /// new connection from node
    NewConnection(NodeId),
    /// connection to node was closed
    ConnectionClosed(NodeId),
    /// A block was received
    ReceivedBlock {
        /// from node id
        node: NodeId,
        /// block
        block: Block,
    },
    /// A block header was received
    ReceivedBlockHeader {
        /// from node id
        source_node_id: NodeId,
        /// header
        header: SignedHeader,
    },
    /// Someone ask for block with given header hash.
    AskedForBlocks {
        /// node id
        node: NodeId,
        /// asked blocks
        list: Vec<BlockId>,
    },
    /// That node does not have this block
    BlockNotFound {
        /// node id
        node: NodeId,
        /// block id
        block_id: BlockId,
    },
    /// received operations from node
    ReceivedOperations {
        /// node id
        node: NodeId,
        /// operations
        operations: Vec<SignedOperation>,
    },
    /// received endorsements from node
    ReceivedEndorsements {
        /// node id
        node: NodeId,
        /// Endorsements
        endorsements: Vec<SignedEndorsement>,
    },
}

/// Network management command
#[derive(Debug)]
pub enum NetworkManagementCommand {}
