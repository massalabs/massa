use crate::error::CommunicationError;
use crate::network::network_controller::NetworkController;
use async_trait::async_trait;
use crypto::{hash::Hash, signature::PublicKey};
use models::block::Block;
use serde::{Deserialize, Serialize};

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

#[derive(Clone, Debug)]
pub enum ProtocolEventType {
    ReceivedTransaction(String),
    ReceivedBlock(Block),
    AskedBlock(Hash),
}

#[derive(Clone, Debug)]
pub struct ProtocolEvent(pub NodeId, pub ProtocolEventType);

#[async_trait]
pub trait ProtocolController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type NetworkControllerT: NetworkController;
    async fn wait_event(&mut self) -> Result<ProtocolEvent, CommunicationError>;
    async fn stop(mut self) -> Result<(), CommunicationError>;
    async fn propagate_block(
        &mut self,
        block: &Block,
        exclude_node: Option<NodeId>,
        restrict_to_node: Option<NodeId>,
    ) -> Result<(), CommunicationError>;
}
