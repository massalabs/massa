use std::{num::NonZeroUsize, sync::Arc, time::Instant};

use lru::LruCache;
use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use parking_lot::RwLock;
use peernet::peer_id::PeerId;

pub struct BlockCache {
    pub checked_headers: LruCache<BlockId, SecuredHeader>,
    //TODO: Add the val true fal as a value
    pub blocks_known_by_peer: LruCache<PeerId, (LruCache<BlockId, (bool, Instant)>, Instant)>,
    pub max_node_known_blocks_size: NonZeroUsize,
}

impl BlockCache {
    pub fn insert_blocks_known(
        &mut self,
        from_peer_id: &PeerId,
        block_ids: &[BlockId],
        val: bool,
        timeout: Instant,
    ) {
        let (blocks, _) = self
            .blocks_known_by_peer
            .get_or_insert_mut(*from_peer_id, || {
                (
                    LruCache::new(self.max_node_known_blocks_size),
                    Instant::now(),
                )
            });
        for block_id in block_ids {
            blocks.put(*block_id, (val, timeout));
        }
    }
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
