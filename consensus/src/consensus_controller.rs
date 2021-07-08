use async_trait::async_trait;
use communication::protocol::protocol_controller::ProtocolController;
use crypto::hash::Hash;
use models::block::Block;

use crate::{block_graph::BlockGraphExport, error::ConsensusError};

#[derive(Clone, Debug)]
pub enum ConsensusEvent {}

#[async_trait]
pub trait ConsensusController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type ProtocolControllerT: ProtocolController;
    async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError>;
    async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError>;
    async fn get_active_block(&self, hash: Hash) -> Result<Option<Block>, ConsensusError>;
}
