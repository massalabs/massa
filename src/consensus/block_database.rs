use super::{config::ConsensusConfig, random_selector::RandomSelector};
use crate::{
    crypto::{hash::Hash, signature::SignatureEngine},
    structures::block::Block,
    structures::block::BlockHeader,
};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub struct BlockDatabase {
    cfg: ConsensusConfig,
    pub active_blocks: Vec<HashMap<Hash, Block>>,
    pub seen_blocks: HashSet<Hash>,
}

impl BlockDatabase {
    pub fn new(cfg: &ConsensusConfig) -> Self {
        BlockDatabase {
            cfg: cfg.clone(),
            active_blocks: vec![HashMap::new(); cfg.thread_count as usize],
            seen_blocks: HashSet::new(),
        }
    }

    pub fn create_block(&self, val: String) -> Block {
        let secp = SignatureEngine::new();
        let private_key = SignatureEngine::generate_random_private_key();
        let public_key = secp.derive_public_key(&private_key);
        let example_hash = Hash::hash(&val.as_bytes());

        let header = BlockHeader {
            creator: public_key,
            thread_number: 0,
            slot_number: 0,
            roll_number: 0,
            parents: Vec::new(),
            endorsements: Vec::new(),
            out_ledger_hash: example_hash,
            operation_merkle_root: example_hash,
        };

        let header_hash = {
            let mut serializer = flexbuffers::FlexbufferSerializer::new();
            header
                .serialize(&mut serializer)
                .expect("could not serialize header");
            Hash::hash(serializer.view())
        };

        Block {
            header,
            operations: Vec::new(),
            signature: secp.sign(&header_hash, &private_key),
        }
    }

    pub fn is_block_valid(
        &self,
        header_hash: &Hash,
        block: &Block,
        selector: &mut RandomSelector,
    ) -> bool {
        // check values
        if block.header.parents.len() != (self.cfg.thread_count as usize) {
            return false;
        }
        if block.header.thread_number >= self.cfg.thread_count {
            return false;
        }
        // TODO if block.header.endorsements.len() > xxx

        // check if it was their turn
        if block.header.roll_number
            != selector.draw(block.header.thread_number, block.header.slot_number)
        {
            return false;
        }

        // check signature
        let sig = SignatureEngine::new();
        if !sig.verify(&header_hash, &block.signature, &block.header.creator) {
            return false;
        }

        true
    }

    // - Check if block is new
    // - Check if block is valid
    // if yes insert block in blocks and returns true
    pub fn acknowledge_new_block(&mut self, block: &Block, selector: &mut RandomSelector) -> bool {
        // compute header hash
        let header_hash = {
            let mut serializer = flexbuffers::FlexbufferSerializer::new();
            block
                .header
                .serialize(&mut serializer)
                .expect("could not serialize header");
            Hash::hash(serializer.view())
        };

        // check if we already have the block
        if self.seen_blocks.contains(&header_hash) {
            return false;
        }
        if block.header.thread_number >= self.cfg.thread_count {
            self.seen_blocks.insert(header_hash);
            return false;
        }
        if self.active_blocks[block.header.thread_number as usize].contains_key(&header_hash) {
            return false;
        }

        // check block validity
        if !self.is_block_valid(&header_hash, &block, selector) {
            self.seen_blocks.insert(header_hash);
            return false;
        }

        self.active_blocks[block.header.thread_number as usize].insert(header_hash, block.clone());
        true
    }
}
