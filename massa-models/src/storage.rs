use crate::prehash::Map;
use crate::{Block, BlockId};
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::debug;

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
        // TODO: first check, and allow for, an already stored header for the block.
        let stored_block = StoredBlock {
            block,
            serialized,
            serialized_header: None,
        };
        let to_store = Arc::new(RwLock::new(stored_block));
        //debug!("Store block {} in storage", block_id);
        let mut blocks = self.blocks.write();
        blocks.insert(block_id, to_store);
    }

    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<StoredBlock>>> {
        let blocks = self.blocks.read();
        if let Some(block) = blocks.get(block_id) {
            return Some(Arc::clone(block));
        }
        None
    }
}
