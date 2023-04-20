use std::sync::Arc;

use lru::LruCache;
use massa_models::block_id::BlockId;
use parking_lot::RwLock;
use peernet::peer_id::PeerId;

pub struct BlockCache {
    pub checked_header: LruCache<BlockId, ()>,
    pub blocks_known_by_peer: LruCache<PeerId, LruCache<BlockId, ()>>,
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
