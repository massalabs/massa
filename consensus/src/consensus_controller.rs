use std::{collections::HashMap, net::IpAddr};

use crate::{block_graph::BlockGraphExport, error::ConsensusError};
use async_trait::async_trait;
use communication::network::PeerInfo;
use communication::protocol::protocol_controller::ProtocolController;
use crypto::{hash::Hash, signature::PublicKey};
use models::block::Block;

/// Consensus events that can be sent out.
#[derive(Clone, Debug)]
pub enum ConsensusEvent {}

/// To speak with consensus controller.
#[async_trait]
pub trait ConsensusControllerInterface
where
    Self: Send + Clone + Sync + Unpin + std::fmt::Debug,
{
    /// Retrieves whole block graph.
    async fn get_block_graph_status(&self) -> Result<BlockGraphExport, ConsensusError>;
    /// Retrives active block with given hash.
    ///
    /// # Argument
    /// * hash: hash of the block we want.
    async fn get_active_block(&self, hash: Hash) -> Result<Option<Block>, ConsensusError>;
    /// Retrives known peers.
    async fn get_peers(&self) -> Result<HashMap<IpAddr, PeerInfo>, ConsensusError>;
    /// Retrives slots with selected staker for interval.
    ///
    /// # Arguments
    /// * start_slot: begining of the considered interval.
    /// * end_slot: end of the considered interval.
    async fn get_selection_draws(
        &self,
        start_slot: (u64, u8),
        end_slot: (u64, u8),
    ) -> Result<Vec<((u64, u8), PublicKey)>, ConsensusError>;
}

/// Controls consensus work.
#[async_trait]
pub trait ConsensusController
where
    Self: Send + Sync + Unpin + std::fmt::Debug,
{
    type ProtocolControllerT: ProtocolController;
    type ConsensusControllerInterfaceT: ConsensusControllerInterface;
    /// Generates associated interface.
    fn get_interface(&self) -> Self::ConsensusControllerInterfaceT;
    /// Waits for next consensus event.
    async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError>;
}
