//! Only public for the protocol worker.
//! Contains all what we know for each node we are connected or we used to
//! know in the network.
//!
//! # Operations
//! Same as for wanted/known blocks, we remember here in cache which node asked
//! for operations and which operations he seem to already know.

use massa_models::operation::OperationPrefixId;
use massa_models::prehash::{CapacityAllocator, PreHashMap};
use massa_models::{block_id::BlockId, endorsement::EndorsementId};
use massa_protocol_exports::ProtocolConfig;
use tokio::time::Instant;

use crate::cache::LinearHashCacheSet;

/// Information about a node we are connected to,
/// essentially our view of its state.
#[derive(Debug, Clone)]
pub(crate) struct NodeInfo {
    /// The blocks the node "knows about",
    /// defined as the one the node propagated headers to us for.
    pub(crate) known_blocks: PreHashMap<BlockId, (bool, Instant)>,
    /// Blocks we asked that node for
    pub asked_blocks: PreHashMap<BlockId, Instant>,
    /// Instant when the node was added
    pub connection_instant: Instant,
    /// all known operations (prefix-based)
    known_operations: LinearHashCacheSet<OperationPrefixId>,
    /// all known endorsements
    known_endorsements: LinearHashCacheSet<EndorsementId>,
}

impl NodeInfo {
    /// Creates empty node info
    pub fn new(pool_settings: &ProtocolConfig) -> NodeInfo {
        NodeInfo {
            known_blocks: PreHashMap::with_capacity(pool_settings.max_node_known_blocks_size),
            asked_blocks: Default::default(),
            connection_instant: Instant::now(),
            known_operations: LinearHashCacheSet::new(pool_settings.max_node_known_ops_size),
            known_endorsements: LinearHashCacheSet::new(
                pool_settings.max_node_known_endorsements_size,
            ),
        }
    }

    /// Get boolean if block knows about the block and when this information was got
    /// in a option if we don't know if that node knows that block or not
    pub fn get_known_block(&self, block_id: &BlockId) -> Option<&(bool, Instant)> {
        self.known_blocks.get(block_id)
    }

    /// Remove the oldest items from `known_blocks`
    /// to ensure it contains at most `max_node_known_blocks_size` items.
    /// This algorithm is optimized for cases where there are no more than a couple excess items, ideally just one.
    fn remove_excess_known_blocks(&mut self, max_node_known_blocks_size: usize) {
        while self.known_blocks.len() > max_node_known_blocks_size {
            // remove oldest item
            let (&h, _) = self
                .known_blocks
                .iter()
                .min_by_key(|(h, (_, t))| (*t, *h))
                .unwrap(); // never None because is the collection is empty, while loop isn't executed.
            self.known_blocks.remove(&h);
        }
    }

    /// Insert knowledge of a list of blocks in `NodeInfo`
    ///
    /// ## Arguments
    /// * `self`: node info
    /// * `block_ids`: list of blocks
    /// * `val`: if that node knows that block
    /// * `instant`: when that information was created
    /// * `max_node_known_blocks_size`: max size of the knowledge of an other node we want to keep
    pub fn insert_known_blocks(
        &mut self,
        block_ids: &[BlockId],
        val: bool,
        instant: Instant,
        max_node_known_blocks_size: usize,
    ) {
        for block_id in block_ids {
            self.known_blocks.insert(*block_id, (val, instant));
        }
        self.remove_excess_known_blocks(max_node_known_blocks_size);
    }

    pub fn insert_known_endorsements<I: IntoIterator<Item = EndorsementId>>(
        &mut self,
        endorsements: I,
    ) {
        self.known_endorsements.try_extend(endorsements);
    }

    pub fn knows_endorsement(&self, endorsement_id: &EndorsementId) -> bool {
        self.known_endorsements.contains(endorsement_id)
    }

    pub fn insert_known_ops<I: IntoIterator<Item = OperationPrefixId>>(&mut self, ops: I) {
        self.known_operations.try_extend(ops);
    }

    pub fn knows_op(&self, op: &OperationPrefixId) -> bool {
        self.known_operations.contains(op)
    }
}
