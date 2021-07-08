use std::{collections::HashMap, net::IpAddr};

use crate::error::CommunicationError;
use crate::network::{network_controller::NetworkController, PeerInfo};
use async_trait::async_trait;
use crypto::{hash::Hash, signature::PublicKey};
use models::block::Block;
use serde::{Deserialize, Serialize};

/// For now, wraps a public key.
/// Used to uniquely identify a node.
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeId(pub PublicKey);

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::fmt::Debug for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Possible types of events that can happen.
#[derive(Clone, Debug)]
pub enum ProtocolEventType {
    /// A isolated transaction was received.
    ReceivedTransaction(String),
    /// A block was received
    ReceivedBlock(Block),
    /// Someone ask for block with given header hash.
    AskedBlock(Hash),
}

/// Links a protocol event type to the node it came from.
#[derive(Clone, Debug)]
pub struct ProtocolEvent(pub NodeId, pub ProtocolEventType);

/// Manages protocol related events.
#[async_trait]
pub trait ProtocolController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type NetworkControllerT: NetworkController;
    /// Listens for next incomming event.
    async fn wait_event(&mut self) -> Result<ProtocolEvent, CommunicationError>;
    /// Closes everything properly.
    async fn stop(mut self) -> Result<(), CommunicationError>;
    /// Gives the order to propagate given block to every connected peer.
    ///
    ///  # Arguments
    /// * hash: header hash of the given block.
    /// * block: block to propagate.
    async fn propagate_block(
        &mut self,
        hash: Hash,
        block: &Block,
    ) -> Result<(), CommunicationError>;
    /// Returns ips mapped to peerInfo.
    /// For internal use.
    async fn get_peers(&self) -> Result<HashMap<IpAddr, PeerInfo>, CommunicationError>;
}
