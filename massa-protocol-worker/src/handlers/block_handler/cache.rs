use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

/// Maximum number of peers recorded as direct senders of a given block.
/// Bounds the memory used by the attribution record in case many peers send us the same block.
const MAX_SENDERS_PER_BLOCK: usize = 32;

/// Cache on block knowledge by our node and its peers
pub struct BlockCache {
    /// cache of previously checked headers
    pub checked_headers: LruMap<BlockId, SecuredHeader>,
    /// cache of blocks known by peers
    pub blocks_known_by_peer: HashMap<PeerId, LruMap<BlockId, (bool, Instant)>>,
    /// max number of blocks known in peer knowledge cache
    pub max_known_blocks_by_peer: u32,
    /// For each recently seen block, the peers that directly sent us data for it.
    ///
    /// This is attribution data, not propagation state: unlike `blocks_known_by_peer` it only
    /// records peers from which we actually received the block's header or contents, it never
    /// records peers that merely learned about the block from us, and it is not pruned when a
    /// peer disconnects. It is therefore the only sound basis for punitive actions.
    pub block_senders: LruMap<BlockId, HashSet<PeerId>>,
}

impl BlockCache {
    /// Mark a given node's knowledge of a list of blocks
    /// as either known or unknown.
    ///
    /// # Arguments
    ///
    /// * `from_peer_id` - The peer id of the peer to mark
    /// * `block_ids` - The list of block ids to mark
    /// * `known` - Whether the blocks are known or unknown by the peer
    pub fn insert_peer_known_block(
        &mut self,
        from_peer_id: &PeerId,
        block_ids: &[BlockId],
        known: bool,
    ) {
        let now = Instant::now();
        let known_blocks = self
            .blocks_known_by_peer
            .entry(*from_peer_id)
            .or_insert_with(|| LruMap::new(ByLength::new(self.max_known_blocks_by_peer)));
        for block_id in block_ids {
            known_blocks.insert(*block_id, (known, now));
        }
    }

    /// Record that a peer directly sent us data (header or contents) for a given block.
    ///
    /// # Arguments
    ///
    /// * `block_id` - The block the data belongs to
    /// * `from_peer_id` - The peer that sent us the data
    pub fn insert_block_sender(&mut self, block_id: &BlockId, from_peer_id: &PeerId) {
        let senders = self
            .block_senders
            .get_or_insert(*block_id, HashSet::new)
            .expect("failed to insert block senders entry");
        // bound the number of recorded senders per block
        if senders.len() < MAX_SENDERS_PER_BLOCK {
            senders.insert(*from_peer_id);
        }
    }

    /// Get the peers that directly sent us data for a given block.
    pub fn get_block_senders(&self, block_id: &BlockId) -> Vec<PeerId> {
        self.block_senders
            .peek(block_id)
            .map(|senders| senders.iter().copied().collect())
            .unwrap_or_default()
    }
}

impl BlockCache {
    pub fn new(max_known_blocks: u32, max_known_blocks_by_peer: u32) -> Self {
        Self {
            checked_headers: LruMap::new(ByLength::new(max_known_blocks)),
            blocks_known_by_peer: HashMap::new(),
            max_known_blocks_by_peer,
            block_senders: LruMap::new(ByLength::new(max_known_blocks)),
        }
    }

    pub fn update_cache(&mut self, peers_connected: &HashSet<PeerId>) {
        // Remove disconnected peers from cache
        self.blocks_known_by_peer
            .retain(|peer_id, _| peers_connected.contains(peer_id));

        // Add new connected peers to cache
        for peer_id in peers_connected {
            match self.blocks_known_by_peer.entry(*peer_id) {
                std::collections::hash_map::Entry::Occupied(_) => {}
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(LruMap::new(ByLength::new(self.max_known_blocks_by_peer)));
                }
            }
        }
    }
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
