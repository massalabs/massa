//! Declaration of the public and internal Events and Commands used by
//! `massa-network` that allow external communication with other nodes.
//!
//! # Operations workflow
//! An operation batch can be send or received by the massa node. Which
//! operation is asked is managed by the `NetworkWorker`.
//!
//! Other modules has the access to all commands but the usage if they want to
//! send operation that they just noticed, they should use the command
//! [NetworkCommand::SendOperationAnnouncements].
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
//! The network will inform the node that new operations can transites with
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
//! remember wich node know what.

use crate::{BootstrapPeers, ConnectionClosureReason, Peers};
use massa_models::{
    composite::PubkeySig,
    node::NodeId,
    operation::{OperationIds, Operations},
    stats::NetworkStats,
    Block, BlockId, SignedEndorsement, SignedHeader,
};
use std::{collections::HashMap, net::IpAddr};
use tokio::sync::oneshot;

#[derive(Clone, Debug)]
pub enum NodeCommand {
    /// Send given peer list to node.
    SendPeerList(Vec<IpAddr>),
    /// Send that block to node.
    SendBlock(Block),
    /// Send the header of a block to a node.
    SendBlockHeader(SignedHeader),
    /// Ask for a block from that node.
    AskForBlocks(Vec<BlockId>),
    /// Close the node worker.
    Close(ConnectionClosureReason),
    /// Block not found
    BlockNotFound(BlockId),
    /// Send full Operations (send to a node that previously asked for)
    SendOperations(Operations),
    /// Send a batch of operation ids
    SendOperationAnnouncements(OperationIds),
    /// Ask for a set of operations
    AskForOperations(OperationIds),
    /// Endorsements
    SendEndorsements(Vec<SignedEndorsement>),
}

/// Event types that node worker can emit
/// Append on receive something from inside and outside.
/// Outside init with `Received` prefix.
#[derive(Clone, Debug)]
pub enum NodeEventType {
    /// Node we are connected to asked for advertised peers
    AskedPeerList,
    /// Node we are connected to sent peer list
    ReceivedPeerList(Vec<IpAddr>),
    /// Node we are connected to sent block
    ReceivedBlock(Block),
    /// Node we are connected to sent block header
    ReceivedBlockHeader(SignedHeader),
    /// Node we are connected to asks for a block.
    ReceivedAskForBlocks(Vec<BlockId>),
    /// Didn't found given block,
    BlockNotFound(BlockId),
    /// Received full operation
    ReceivedOperations(Operations),
    /// Received an operation id batch announcing new operations
    ReceivedOperationAnnouncements(OperationIds),
    /// Receive a list of wanted operations
    ReceivedAskForOperations(OperationIds),
    /// Receive a set of endorsement
    ReceivedEndorsements(Vec<SignedEndorsement>),
}

/// Events node worker can emit.
/// Events are a tuple linking a node id to an event type
#[derive(Clone, Debug)]
pub struct NodeEvent(pub NodeId, pub NodeEventType);

/// Commands that the worker can execute
#[derive(Debug)]
pub enum NetworkCommand {
    /// Ask for a block to a node.
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
    GetBootstrapPeers(oneshot::Sender<BootstrapPeers>),
    /// Ban a peer by his node id
    Ban(NodeId),
    /// Ban a list of peer by their ip address
    BanIp(Vec<IpAddr>),
    /// Unban a list of peer by their ip address
    Unban(Vec<IpAddr>),
    /// Send a message that a block is not found to a node
    BlockNotFound {
        node: NodeId,
        block_id: BlockId,
    },
    /// Send endorsements to a node
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
    /// Send a batch of full operations
    SendOperations {
        node: NodeId,
        operations: Operations,
    },
    /// Send operation ids batch to a node
    SendOperationAnnouncements {
        to_node: NodeId,
        batch: OperationIds,
    },
    /// Ask for operation
    AskForOperations {
        to_node: NodeId,
        wishlist: OperationIds,
    },
    /// Whitelist a list of IpAddr
    Whitelist(Vec<IpAddr>),
    /// Remove from whitelist a list of IpAddr
    RemoveFromWhitelist(Vec<IpAddr>),
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
    /// Receive previously asked Operation
    ReceivedOperations {
        node: NodeId,
        operations: Operations,
    },
    /// Receive a list of OperationId
    ReceivedOperationAnnouncements {
        node: NodeId,
        operation_ids: OperationIds,
    },
    /// Receive a list of asked operations from `node`
    ReceiveAskForOperations {
        node: NodeId,
        operation_ids: OperationIds,
    },
    ReceivedEndorsements {
        node: NodeId,
        endorsements: Vec<SignedEndorsement>,
    },
}

#[derive(Debug)]
pub enum NetworkManagementCommand {}
