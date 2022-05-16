//! This crate is used to share blocks across the node

#![warn(missing_docs)]

use massa_logging::massa_trace;
use massa_models::prehash::Map;
use massa_models::{Block, BlockId};
use parking_lot::RwLock;
use std::collections::hash_map::Entry;
use std::sync::Arc;

/// Stored block: block + serialized block + (serialized header)
#[derive(Debug)]
pub struct StoredBlock {
    /// The block.
    pub block: Block,
    /// The serialized representation of the block.
    pub serialized: Vec<u8>,
    /// The serialized representation of the header, if any.
    /// Note: the header is written as part of propagation of headers.
    pub serialized_header: Option<Vec<u8>>,
}

/// A storage of block, shared by various components.
#[derive(Clone, Default)]
pub struct Storage {
    blocks: Arc<RwLock<Map<BlockId, Arc<RwLock<StoredBlock>>>>>,
}

impl Storage {
    /// Store a block, along with it's serialized representation.
    pub fn store_block(&self, block_id: BlockId, block: Block, serialized: Vec<u8>) {
        massa_trace!("storage.storage.store_block", { "block_id": block_id });
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

    /// Get a (mutable) reference to the stored block.
    pub fn retrieve_block(&self, block_id: &BlockId) -> Option<Arc<RwLock<StoredBlock>>> {
        massa_trace!("storage.storage.retrieve_block", { "block_id": block_id });
        let blocks = self.blocks.read();
        blocks.get(block_id).map(Arc::clone)
    }

    /// Remove a list of blocks from storage.
    pub fn remove_blocks(&self, block_ids: &[BlockId]) {
        massa_trace!("storage.storage.remove_blocks", { "block_ids": block_ids });
        let mut blocks = self.blocks.write();
        for id in block_ids {
            blocks.remove(id);
        }
    }
}
