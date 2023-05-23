use std::{sync::Arc, time::Instant};

use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};

pub struct BlockCache {
    pub checked_headers: LruMap<BlockId, SecuredHeader>,
    #[allow(clippy::type_complexity)]
    pub blocks_known_by_peer: LruMap<PeerId, (LruMap<BlockId, (bool, Instant)>, Instant)>,
    pub max_known_blocks_by_peer: u32,
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
            .get_or_insert(from_peer_id.clone(), || {
                (
                    LruMap::new(ByLength::new(self.max_known_blocks_by_peer)),
                    Instant::now(),
                )
            })
            .ok_or(())
            .expect("blocks_known_by_peer limit reached");
        for block_id in block_ids {
            blocks.insert(*block_id, (val, timeout));
        }
    }
}

impl BlockCache {
    pub fn new(max_known_blocks: u32, max_known_blocks_by_peer: u32) -> Self {
        Self {
            checked_headers: LruMap::new(ByLength::new(max_known_blocks)),
            blocks_known_by_peer: LruMap::new(ByLength::new(max_known_blocks_by_peer)),
            max_known_blocks_by_peer,
        }
    }
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
