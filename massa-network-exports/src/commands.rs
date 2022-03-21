use std::{collections::HashMap, net::IpAddr};

use crate::Peers;
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
        list: HashMap<NodeId, Vec<BlockId>>,
    },
    /// Send that block to node.
    SendBlock {
        node: NodeId,
        block: Block,
    },
    /// Send a header to a node.
    SendBlockHeader {
        node: NodeId,
        header: SignedHeader,
    },
    // (PeerInfo, Vec <(NodeId, bool)>) peer info + list of associated Id nodes in connection out (true)
    GetPeers(oneshot::Sender<Peers>),
    GetBootstrapPeers(oneshot::Sender<Vec<IpAddr>>),
    Ban(NodeId),
    BanIp(Vec<IpAddr>),
    Unban(Vec<IpAddr>),
    BlockNotFound {
        node: NodeId,
        block_id: BlockId,
    },
    SendOperations {
        node: NodeId,
        operations: Vec<SignedOperation>,
    },
    SendEndorsements {
        node: NodeId,
        endorsements: Vec<SignedEndorsement>,
    },
    NodeSignMessage {
        msg: Vec<u8>,
        response_tx: oneshot::Sender<PubkeySig>,
    },
    GetStats {
        response_tx: oneshot::Sender<NetworkStats>,
    },
}

#[derive(Debug)]
pub enum NetworkEvent {
    NewConnection(NodeId),
    ConnectionClosed(NodeId),
    /// A block was received
    ReceivedBlock {
        node: NodeId,
        block: Block,
    },
    /// A block header was received
    ReceivedBlockHeader {
        source_node_id: NodeId,
        header: SignedHeader,
    },
    /// Someone ask for block with given header hash.
    AskedForBlocks {
        node: NodeId,
        list: Vec<BlockId>,
    },
    /// That node does not have this block
    BlockNotFound {
        node: NodeId,
        block_id: BlockId,
    },
    ReceivedOperations {
        node: NodeId,
        operations: Vec<SignedOperation>,
    },
    ReceivedEndorsements {
        node: NodeId,
        endorsements: Vec<SignedEndorsement>,
    },
}

#[derive(Debug)]
pub enum NetworkManagementCommand {}
