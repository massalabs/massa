use std::{sync::Arc, time::Instant};

use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use massa_protocol_exports::PeerId;
use parking_lot::RwLock;
use schnellru::{ByLength, LruMap};
use tracing::log::warn;

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
        let Ok((blocks, _)) = self
            .blocks_known_by_peer
            .get_or_insert(from_peer_id.clone(), || {
                (
                    LruMap::new(ByLength::new(self.max_known_blocks_by_peer)),
                    Instant::now(),
                )
            })
            .ok_or(()) else {
                warn!("blocks_known_by_peer limit reached");
                return;
            };
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

    pub fn update_cache(
        &mut self,
        peers_connected: HashSet<PeerId>,
        max_known_blocks_by_peer: u32,
    ) {
        let peers: Vec<PeerId> = self
            .max_known_blocks_by_peer
            .iter()
            .map(|(id, _)| id.clone())
            .collect();

        // Clean shared cache if peers do not exist anymore
        for peer_id in peers {
            if !peers_connected.contains(&peer_id) {
                self.max_known_blocks_by_peer.remove(&peer_id);
            }
        }

        // Add new potential peers
        for peer_id in peers_connected {
            if self.max_known_blocks_by_peer.peek(&peer_id).is_none() {
                self.max_known_blocks_by_peer.insert(
                    peer_id.clone(),
                    LruMap::new(ByLength::new(max_known_blocks_by_peer)),
                );
            }
        }
    }
}

pub type SharedBlockCache = Arc<RwLock<BlockCache>>;
