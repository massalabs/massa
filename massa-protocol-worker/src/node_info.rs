//! Only public for the protocol worker.
//! Contains all what we know for each node we are connected or we used to
//! know in the network.
//!
//! # Operations
//! Same as for wanted/known blocks, we remember here in cache which node asked
//! for operations and which operations he seem to already know.

use massa_models::cache::HashCacheSet;
use massa_models::{block::BlockId, endorsement::EndorsementId, operation::OperationId};
use massa_protocol_exports::ProtocolConfig;

/// Information on what we thing a node we are connected to knows,
#[derive(Debug, Clone)]
pub(crate) struct NodeInfo {
    /// The blocks the node knows about
    pub(crate) known_blocks: HashCacheSet<BlockId>,
    /// all known operations
    pub(crate) known_operations: HashCacheSet<OperationId>,
    /// all known endorsements
    pub(crate) known_endorsements: HashCacheSet<EndorsementId>,
}

impl NodeInfo {
    /// Creates an empty node info
    pub fn new(pool_settings: &ProtocolConfig) -> NodeInfo {
        NodeInfo {
            known_blocks: HashCacheSet::new(pool_settings.max_node_known_blocks_size),
            known_operations: HashCacheSet::new(pool_settings.max_node_known_ops_size),
            known_endorsements: HashCacheSet::new(pool_settings.max_node_known_endorsements_size),
        }
    }
}
