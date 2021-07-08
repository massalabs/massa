use crate::crypto::hash::Hash;
use crate::structures::block::Block;
use std::collections::HashMap;

pub struct BlockDatabase(pub HashMap<Hash, Block>);

impl BlockDatabase {
    pub fn new() -> Self {
        BlockDatabase(HashMap::new())
    }

    pub fn create_block(&self, val: String) -> Block {
        let data_bytes = val.as_bytes().to_vec();
        let mut engine = Hash::engine();
        engine.input(&data_bytes);
        let hash = Hash::from_engine(engine);
        Block { hash, val }
    }

    pub fn is_block_valid(&self, block: &Block) -> bool {
        // TODO check if block fits inside block_db
        let data = block.val.as_bytes().to_vec();
        let mut engine = Hash::engine();
        engine.input(&data);
        let hashed_val = Hash::from_engine(engine);
        block.hash == hashed_val
    }

    // - Check if block is new
    // - Check if block is valid
    // if yes insert block in blocks and returns true
    pub fn acknowledge_new_block(&mut self, block: &Block) -> bool {
        if self.is_block_valid(&block) {
            if let None = self.0.get(&block.hash) {
                // block is not already in block_db
                self.0.insert(block.hash, block.clone());
                return true;
            }
        }
        false
    }
}
