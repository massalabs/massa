//! Declaration of the public and internal Events and Commands used by
//! `massa-network` that allow external communication with other nodes.
//!
//! # Operations work flow
//! An operation batch can be send or received by the massa node. Which
//! operation is asked is managed by the `NetworkWorker`.
//!
//! Other modules has the access to all commands but the usage if they want to
//! send operation that they just noticed, they should use the command
//! `[NetworkCommand::SendOperationAnnouncements]`.
//!
//! ```txt
//! OurNode      ProtocolWorker      Network & NodeWorker
//!    |               |                |
//!    +------------------------------------------- Creation of some operations
//!    #               |                |           Extends the pool and
//!    #               |                |
//!    +-------------->|                |           Forward to Protocol
//!    |               +--------------->|           Forward to Network, then Node
//!    |               |                #
//!    |               |                #           Propagate the batch of OperationIds through the network
//! ```
//!
//! When receiving an operation batch from the network, the `NodeWorker` will
//! inform the `ProtocolWorker` that some potentials new operations are in
//! transit and can be requested.
//!
//! The network will inform the node that new operations can transit with
//! the propagation method `SendOperationAnnouncements` that we defined just before.
//! Then, the node will manage if he ask or not the operations inside the
//! `ProtocolWorker` on node-worker event `ReceivedOperationAnnouncements`
//!
//! ```txt
//! Asking for operations
//! ---
//!
//! NodeWorker      NetworkWorker          ProtocolWorker
//!    |               |                         |
//!    +------------------------------------------------------- Receive a batch of annoucemnt
//!    .               |                         |
//!    +-------------->|                         |              NetworkWorker react on previous event. Forward to protocol.
//!    |               +------------------------>#              - Check in the protocol if we already have operations
//!    |               |                         #              or not. Build the vector of requirement.
//!    |               |                         #              - Update the `NodeInfo` of the sender.
//!    |               |                         #              - Use the propagation algorithm
//!    |               |<------------------------+
//!    |<--------------+                         |
//!    |               |                         |
//!    |               |                         |
//!    |               |                         |              > Ask to the node that sent the batch a list of operations
//!    |               |                         |              > that we don't already know with `NodeCommand::AskForOperations`.
//!    |               |                         |
//!    +-------------->|                         |
//!    |               +------------------------>#
//!    |               |                         #
//!    |               |                         #              > Receive the full operations inside the structure
//!    |               |                         #              > `AskedOperation`.
//!    |               |                         #
//!    |               |                         #              Update local state and the `NodeInfo` of the sender if required.
//!    |               |                         |
//!    |               |<------------------------+              Once we received and store the operation, we can propagate it.
//!    |<--------------+                         |
//!    |               |                         |
//!    |               |                         |              > Propagate the batch through the network (send to nodes
//!    |               |                         |              > that don't already know the local operations (we suppose that
//!    |               |                         |              > from previous discussions with distant node)
//! ```
//!
//! Look at `massa-protocol-worker/src/node-info.rs` to look further how we
//! remember which node know what.

use crate::{BootstrapPeers, ConnectionClosureReason, Peers};
use massa_models::{
    composite::PubkeySig,
    node::NodeId,
    operation::{OperationIds, OperationPrefixIds, Operations},
    stats::NetworkStats,
    BlockId, WrappedBlock, WrappedEndorsement, WrappedHeader,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::IpAddr};
use tokio::sync::oneshot;

/// network command
#[derive(Clone, Debug)]
pub enum NodeCommand {
    /// Send given peer list to node.
    SendPeerList(Vec<IpAddr>),
    /// Send that block to node.
    SendBlock(BlockId),
    /// Send info about the content of a block to a node.
    SendBlockInfo {
        /// block id
        block_id: BlockId,
        /// List of operations
        operation_list: OperationIds,
    },
    /// Send the header of a block to a node.
    SendBlockHeader(BlockId),
    /// Ask for info on a list of blocks.
    AskForBlocks(Vec<(BlockId, AskForBlocksInfo)>),
    /// Reply with info on a list of blocks.
    ReplyForBlocks(Vec<(BlockId, ReplyForBlocksInfo)>),
    /// Close the node worker.
    Close(ConnectionClosureReason),
    /// Block not found
    BlockNotFound(BlockId),
    /// Send full Operations (send to a node that previously asked for)
    SendOperations(OperationIds),
    /// Send a batch of operation ids
    SendOperationAnnouncements(OperationPrefixIds),
    /// Ask for a set of operations
    AskForOperations(OperationPrefixIds),
    /// Endorsements
    SendEndorsements(Vec<WrappedEndorsement>),
}

/// Event types that node worker can emit
/// Append on receive something from inside and outside.
/// Outside initialization with `Received` prefix.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum NodeEventType {
    /// Node we are connected to asked for advertised peers
    AskedPeerList,
    /// Node we are connected to sent peer list
    ReceivedPeerList(Vec<IpAddr>),
    /// Node we are connected to sent block
    ReceivedBlock(WrappedBlock),
    /// Node we are connected to sent block header
    ReceivedBlockHeader(WrappedHeader),
    /// Node we are connected asked for info on a list of blocks.
    ReceivedAskForBlocks(Vec<(BlockId, AskForBlocksInfo)>),
    /// Node we are connected sent info on a list of blocks.
    ReceivedReplyForBlocks(Vec<(BlockId, ReplyForBlocksInfo)>),
    /// Didn't found given block,
    BlockNotFound(BlockId),
    /// Info about the contents of a block.
    ReceivedBlockInfo {
        block_id: BlockId,
        operation_list: OperationIds,
    },
    /// Received full operations.
    ReceivedOperations(Operations),
    /// Received an operation id batch announcing new operations
    ReceivedOperationAnnouncements(OperationPrefixIds),
    /// Receive a list of wanted operations
    ReceivedAskForOperations(OperationPrefixIds),
    /// Receive a set of endorsement
    ReceivedEndorsements(Vec<WrappedEndorsement>),
}

/// Events node worker can emit.
/// Events are a tuple linking a node id to an event type
#[derive(Clone, Debug)]
pub struct NodeEvent(pub NodeId, pub NodeEventType);

/// Ask for the info about a block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum AskForBlocksInfo {
    // The info about the block is required(list of operations ids).
    #[default]
    Info,
    // The actual operations are required.
    Operations(OperationIds),
}

/// Reply with the content required for a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplyForBlocksInfo {
    // The info about the block is required(list of operations ids).
    Info(OperationIds),
    // The actual operations required.
    Operations(OperationIds),
    // Block not found
    NotFound,
}

/// Commands that the worker can execute
#[derive(Debug)]
pub enum NetworkCommand {
    /// Ask for a block to a node.
    AskForBlocks {
        /// node to block ids
        list: HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>>,
    },
    /// Send that block to node.
    SendBlock {
        /// to node id
        node: NodeId,
        /// block id
        block_id: BlockId,
    },
    /// Send info about the content of a block to a node.
    SendBlockInfo {
        /// to node id
        node: NodeId,
        /// block id
        info: Vec<(BlockId, ReplyForBlocksInfo)>,
    },
    /// Send a header to a node.
    SendBlockHeader {
        /// to node id
        node: NodeId,
        /// block id
        block_id: BlockId,
    },
    /// `(PeerInfo, Vec <(NodeId, bool)>) peer info + list` of associated Id nodes in connection out (true)
    GetPeers(oneshot::Sender<Peers>),
    /// get peers for bootstrap server
    GetBootstrapPeers(oneshot::Sender<BootstrapPeers>),
    /// Ban a list of peer by their node id
    NodeBanByIds(Vec<NodeId>),
    /// Ban a list of peer by their ip address
    NodeBanByIps(Vec<IpAddr>),
    /// Unban a list of peer by their node id
    NodeUnbanByIds(Vec<NodeId>),
    /// Unban a list of peer by their ip address
    NodeUnbanByIps(Vec<IpAddr>),
    /// Send a message that a block is not found to a node
    BlockNotFound {
        /// to node id
        node: NodeId,
        /// block id
        block_id: BlockId,
    },
    /// Send endorsements to a node
    SendEndorsements {
        /// to node id
        node: NodeId,
        /// endorsements
        endorsements: Vec<WrappedEndorsement>,
    },
    /// sign message with our node keypair (associated to node id)
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
    /// Send a batch of full operations
    SendOperations {
        /// to node id
        node: NodeId,
        /// operations
        operations: OperationIds,
    },
    /// Send operation ids batch to a node
    SendOperationAnnouncements {
        /// to node id
        to_node: NodeId,
        /// batch of operation ids
        batch: OperationPrefixIds,
    },
    /// Ask for operation
    AskForOperations {
        /// to node id
        to_node: NodeId,
        /// operation ids in the wish list
        wishlist: OperationPrefixIds,
    },
    /// Whitelist a list of `IpAddr`
    Whitelist(Vec<IpAddr>),
    /// Remove from whitelist a list of `IpAddr`
    RemoveFromWhitelist(Vec<IpAddr>),
}

/// A node replied with info about a block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockInfoReply {
    // The info about the block is required(list of operations ids).
    Info(OperationIds),
    // The actual operations required.
    Operations(Operations),
    // Block not found
    NotFound,
}

/// network event
#[allow(clippy::large_enum_variant)]
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
        block: WrappedBlock,
    },
    /// Info about a block was received
    ReceivedBlockInfo {
        /// from node id
        node: NodeId,
        /// block
        info: Vec<(BlockId, BlockInfoReply)>,
    },
    /// A block header was received
    ReceivedBlockHeader {
        /// from node id
        source_node_id: NodeId,
        /// header
        header: WrappedHeader,
    },
    /// Someone ask for block with given header hash.
    AskedForBlocks {
        /// node id
        node: NodeId,
        /// asked blocks
        list: Vec<(BlockId, AskForBlocksInfo)>,
    },
    /// That node does not have this block
    BlockNotFound {
        /// node id
        node: NodeId,
        /// block id
        block_id: BlockId,
    },
    /// Receive previously asked Operation
    ReceivedOperations {
        /// node id
        node: NodeId,
        /// operations
        operations: Operations,
    },
    /// Receive a list of `OperationId`
    ReceivedOperationAnnouncements {
        /// from node id
        node: NodeId,
        /// operation prefix ids
        operation_prefix_ids: OperationPrefixIds,
    },
    /// Receive a list of asked operations from `node`
    ReceiveAskForOperations {
        /// from node id
        node: NodeId,
        /// operation prefix ids
        operation_prefix_ids: OperationPrefixIds,
    },
    /// received endorsements from node
    ReceivedEndorsements {
        /// node id
        node: NodeId,
        /// Endorsements
        endorsements: Vec<WrappedEndorsement>,
    },
}

/// Network management command
#[derive(Debug)]
pub enum NetworkManagementCommand {}
