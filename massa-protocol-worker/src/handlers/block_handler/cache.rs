use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

/// Cache on block knowledge by our node and its peers
pub struct BlockCache {
    /// cache of previously checked headers
    pub checked_headers: LruMap<BlockId, SecuredHeader>,
    /// cache of blocks known by peers
    pub blocks_known_by_peer: HashMap<PeerId, LruMap<BlockId, (bool, Instant)>>,
    /// max number of blocks known in peer knowledge cache
    pub max_known_blocks_by_peer: u32,
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
    pub fn insert_blocks_known(
        &mut self,
        from_peer_id: &PeerId,
        block_ids: &[BlockId],
        known: bool,
    ) {
        let now = Instant::now();
        let known_blocks = self
            .blocks_known_by_peer
            .entry(from_peer_id.clone())
            .or_insert_with(|| LruMap::new(ByLength::new(self.max_known_blocks_by_peer)));
        for block_id in block_ids {
            known_blocks.insert(*block_id, (known, now));
        }
    }
}

impl BlockCache {
    pub fn new(max_known_blocks: u32, max_known_blocks_by_peer: u32) -> Self {
        Self {
            checked_headers: LruMap::new(ByLength::new(max_known_blocks)),
            blocks_known_by_peer: HashMap::new(),
            max_known_blocks_by_peer,
        }
    }

    pub fn update_cache(&mut self, peers_connected: &HashSet<PeerId>) {
        // Remove disconnected peers from cache
        self.blocks_known_by_peer
            .retain(|peer_id, _| !peers_connected.contains(peer_id));

        // Add new connected peers to cache
        for peer_id in peers_connected {
            match self.blocks_known_by_peer.entry(peer_id.clone()) {
                std::collections::hash_map::Entry::Occupied(_) => {}
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(LruMap::new(ByLength::new(self.max_known_blocks_by_peer)));
                }
            }
        }
    }
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
