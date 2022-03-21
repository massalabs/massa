use crate::prehash::Map;
use crate::{Block, BlockId};
use parking_lot::RwLock;
use std::collections::hash_map::Entry;
use std::sync::Arc;

#[derive(Debug)]
pub struct StoredBlock {
    pub block: Block,
    pub serialized: Vec<u8>,
    pub serialized_header: Option<Vec<u8>>,
}

#[derive(Clone, Default)]
pub struct Storage {
    blocks: Arc<RwLock<Map<BlockId, Arc<RwLock<StoredBlock>>>>>,
}

impl Storage {
    pub fn store_block(&self, block_id: BlockId, block: Block, serialized: Vec<u8>) {
        let mut blocks = self.blocks.write();
        match blocks.entry(block_id) {
            Entry::Occupied(_) => {}
            Entry::Vacant(entry) => {
                let stored_block = StoredBlock {
                    block,
                    serialized,
                    serialized_header: None,
                };
                let to_store = Arc::new(RwLock::new(stored_block));
                entry.insert(to_store);
            }
        }
    }

    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<StoredBlock>>> {
        let blocks = self.blocks.read();
        blocks.get(block_id).map(Arc::clone)
    }
}
