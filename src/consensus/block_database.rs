use super::config::ConsensusConfig;
use crate::crypto::hash::Hash;
use crate::structures::block::Block;
use std::collections::HashMap;

pub struct BlockDatabase(pub Vec<HashMap<Hash, Block>>);

impl BlockDatabase {
    pub fn new(cfg: &ConsensusConfig) -> Self {
        BlockDatabase(vec![HashMap::new(); cfg.thread_count as usize])
    }

    pub fn create_block(&self, val: String) -> Block {
        unimplemented!();
    }

    pub fn is_block_valid(&self, block: &Block) -> bool {
        unimplemented!();
    }

    // - Check if block is new
    // - Check if block is valid
    // if yes insert block in blocks and returns true
    pub fn acknowledge_new_block(&mut self, block: &Block) -> bool {
        unimplemented!();
    }
}
