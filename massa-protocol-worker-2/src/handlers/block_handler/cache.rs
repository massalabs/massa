use std::{num::NonZeroUsize, sync::Arc};

use lru::LruCache;
use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use parking_lot::RwLock;
use peernet::peer_id::PeerId;

pub struct BlockCache {
    pub checked_headers: LruCache<BlockId, SecuredHeader>,
    //TODO: Add the val true fal as a value
    pub blocks_known_by_peer: LruCache<PeerId, LruCache<BlockId, ()>>,
    pub max_node_known_blocks_size: NonZeroUsize,
}

impl BlockCache {
    pub fn insert_blocks_known(&mut self, from_peer_id: &PeerId, block_ids: &[BlockId]) {
        let blocks = self
            .blocks_known_by_peer
            .get_or_insert_mut(*from_peer_id, || {
                LruCache::new(self.max_node_known_blocks_size)
            });
        for block_id in block_ids {
            blocks.put(*block_id, ());
        }
    }
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
