//! Only public for the protocol worker.
//! Contains all what we know for each node we are connected or we used to
//! know in the network.
//!
//! # Operations
//! Same as for wanted/known blocks, we remember here in cache which node asked
//! for operations and which operations he seem to already know.

use massa_models::prehash::{CapacityAllocator, PreHashMap, PreHashSet};
use massa_models::{block::BlockId, endorsement::EndorsementId, operation::OperationId};
use massa_protocol_exports::ProtocolConfig;
use std::collections::VecDeque;
use tokio::time::Instant;

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
    /// all known operations
    pub known_operations: PreHashSet<OperationId>,
    /// Same as `known_operations` but sorted for a premature optimization :-)
    pub known_operations_queue: VecDeque<OperationId>,
    /// all known endorsements
    pub known_endorsements: PreHashSet<EndorsementId>,
    /// Same as `known_endorsements` but sorted for a premature optimization :-)
    pub known_endorsements_queue: VecDeque<EndorsementId>,
}

impl NodeInfo {
    /// Creates empty node info
    pub fn new(pool_settings: &ProtocolConfig) -> NodeInfo {
        NodeInfo {
            known_blocks: PreHashMap::with_capacity(pool_settings.max_node_known_blocks_size),
            asked_blocks: Default::default(),
            connection_instant: Instant::now(),
            known_operations: PreHashSet::<OperationId>::with_capacity(
                pool_settings.max_node_known_ops_size.saturating_add(1),
            ),
            known_operations_queue: VecDeque::with_capacity(
                pool_settings.max_node_known_ops_size.saturating_add(1),
            ),
            known_endorsements: PreHashSet::<EndorsementId>::with_capacity(
                pool_settings.max_node_known_endorsements_size,
            ),
            known_endorsements_queue: VecDeque::with_capacity(
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

    pub fn insert_known_endorsements(
        &mut self,
        endorsements: Vec<EndorsementId>,
        max_endorsements_nb: usize,
    ) {
        for endorsement_id in endorsements.into_iter() {
            if self.known_endorsements.insert(endorsement_id) {
                self.known_endorsements_queue.push_front(endorsement_id);
                if self.known_endorsements_queue.len() > max_endorsements_nb {
                    if let Some(r) = self.known_endorsements_queue.pop_back() {
                        self.known_endorsements.remove(&r);
                    }
                }
            }
        }
    }

    pub fn knows_endorsement(&self, endorsement_id: &EndorsementId) -> bool {
        self.known_endorsements.contains(endorsement_id)
    }

    pub fn insert_known_ops(&mut self, ops: PreHashSet<OperationId>, max_ops_nb: usize) {
        for operation_id in ops.into_iter() {
            if self.known_operations.insert(operation_id) {
                self.known_operations_queue.push_back(operation_id);
                while self.known_operations_queue.len() > max_ops_nb {
                    if let Some(op_id) = self.known_operations_queue.pop_front() {
                        self.known_operations.remove(&op_id);
                    }
                }
            }
        }
    }

    pub fn knows_op(&self, op: &OperationId) -> bool {
        self.known_operations.contains(op)
    }
}
