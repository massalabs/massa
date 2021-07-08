use std::{collections::HashMap, net::IpAddr};

use crate::{block_graph::BlockGraphExport, error::ConsensusError};
use async_trait::async_trait;
use communication::network::PeerInfo;
use communication::protocol::protocol_controller::ProtocolController;
use crypto::{hash::Hash, signature::PublicKey};
use models::block::Block;

#[derive(Clone, Debug)]
pub enum ConsensusEvent {}

#[async_trait]
pub trait ConsensusControllerInterface
where
    Self: Send + Clone + Sync + Unpin + std::fmt::Debug,
{
    async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError>;
    async fn get_active_block(&self, hash: Hash) -> Result<Option<Block>, ConsensusError>;
    async fn get_peers(&self) -> Result<HashMap<IpAddr, PeerInfo>, ConsensusError>;
    async fn get_selection_draws(
        &self,
        start_slot: (u64, u8),
        end_slot: (u64, u8),
    ) -> Result<Vec<((u64, u8), PublicKey)>, ConsensusError>;
}

#[async_trait]
pub trait ConsensusController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type ProtocolControllerT: ProtocolController;
    type ConsensusControllerInterfaceT: ConsensusControllerInterface;
    fn get_interface(&self) -> Self::ConsensusControllerInterfaceT;
    async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError>;
}
