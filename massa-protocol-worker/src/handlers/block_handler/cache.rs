use std::{num::NonZeroUsize, sync::Arc, time::Instant};

use lru::LruCache;
use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use parking_lot::RwLock;
use peernet::peer_id::PeerId;

pub(crate) struct BlockCache {
    pub(crate) checked_headers: LruCache<BlockId, SecuredHeader>,
    #[allow(clippy::type_complexity)]
    pub(crate) blocks_known_by_peer:
        LruCache<PeerId, (LruCache<BlockId, (bool, Instant)>, Instant)>,
    pub(crate) max_known_blocks_by_peer: NonZeroUsize,
}

impl BlockCache {
    pub(crate) fn insert_blocks_known(
        &mut self,
        from_peer_id: &PeerId,
        block_ids: &[BlockId],
        val: bool,
        timeout: Instant,
    ) {
        let (blocks, _) = self
            .blocks_known_by_peer
            .get_or_insert_mut(from_peer_id.clone(), || {
                (LruCache::new(self.max_known_blocks_by_peer), Instant::now())
            });
        for block_id in block_ids {
            blocks.put(*block_id, (val, timeout));
        }
    }
}

impl BlockCache {
    pub(crate) fn new(
        max_known_blocks: NonZeroUsize,
        max_known_blocks_by_peer: NonZeroUsize,
    ) -> Self {
        Self {
            checked_headers: LruCache::new(max_known_blocks),
            blocks_known_by_peer: LruCache::new(max_known_blocks_by_peer),
            max_known_blocks_by_peer,
        }
    }
}

pub(crate) type SharedBlockCache = Arc<RwLock<BlockCache>>;
