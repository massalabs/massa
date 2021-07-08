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
    best_parents: Vec<(u64, Hash)>,
}

fn create_genesis_block(cfg: &ConsensusConfig, thread_number: u8) -> (Hash, Block) {
    let signature_engine = SignatureEngine::new();
    let private_key = cfg.genesis_key;
    let public_key = signature_engine.derive_public_key(&private_key);
    let header = BlockHeader {
        creator: public_key,
        thread_number,
        slot_number: 0,
        roll_number: 0,
        parents: Vec::new(),
        endorsements: Vec::new(),
        out_ledger_hash: Hash::hash("Hello world !".as_bytes()),
        operation_merkle_root: Hash::hash("Hello world !".as_bytes()),
    };
    let header_hash = {
        let mut serializer = flexbuffers::FlexbufferSerializer::new();
        header
            .serialize(&mut serializer)
            .expect("could not serialize header");
        Hash::hash(&serializer.take_buffer())
    };
    let signature = signature_engine.sign(&header_hash, &private_key);
    (
        header_hash,
        Block {
            header,
            signature,
            operations: Vec::new(),
        },
    )
}

impl BlockDatabase {
    pub fn new(cfg: &ConsensusConfig) -> Self {
        let mut active_blocks = vec![HashMap::new(); cfg.thread_count as usize];
        let mut seen_blocks = HashSet::new();
        let mut best_parents: Vec<(u64, Hash)> = Vec::with_capacity(cfg.thread_count as usize);
        for thread in 0u8..cfg.thread_count {
            let (genesis_block_hash, genesis_block) = create_genesis_block(cfg, thread);
            active_blocks[thread as usize].insert(genesis_block_hash, genesis_block);
            seen_blocks.insert(genesis_block_hash);
            best_parents.push((0, genesis_block_hash));
        }
        BlockDatabase {
            cfg: cfg.clone(),
            active_blocks,
            seen_blocks,
            best_parents,
        }
    }

    pub fn create_block(&self, val: String, thread_number: u8, slot_number: u64) -> Block {
        let signature_engine = SignatureEngine::new();
        let (public_key, private_key) =
            match self.cfg.nodes.get(self.cfg.current_node_index as usize) {
                Some((public, private)) => (public.clone(), private.clone()),
                None => panic!("we don't have a private key, we cannot create blocks"),
            };

        let example_hash = Hash::hash(&val.as_bytes());

        let parents = self
            .best_parents
            .iter()
            .map(|(_slot_number, hash)| hash.clone())
            .collect();

        let header = BlockHeader {
            creator: public_key,
            thread_number,
            slot_number,
            roll_number: self.cfg.current_node_index,
            parents,
            endorsements: Vec::new(),
            out_ledger_hash: example_hash,
            operation_merkle_root: example_hash,
        };

        let header_hash = {
            let mut serializer = flexbuffers::FlexbufferSerializer::new();
            header
                .serialize(&mut serializer)
                .expect("could not serialize header");
            Hash::hash(&serializer.take_buffer())
        };

        Block {
            header,
            operations: Vec::new(),
            signature: signature_engine.sign(&header_hash, &private_key),
        }
    }

    pub fn is_nongenesis_block_valid(
        &self,
        header_hash: &Hash,
        block: &Block,
        selector: &mut RandomSelector,
    ) -> bool {
        // check values
        if block.header.parents.len() != (self.cfg.thread_count as usize) {
            return false;
        }
        if block.header.slot_number == 0 || block.header.thread_number >= self.cfg.thread_count {
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
    pub fn acknowledge_new_block(
        &mut self,
        block: &Block,
        selector: &mut RandomSelector,
    ) -> (bool, Hash) {
        // compute header hash
        let header_hash = {
            let mut serializer = flexbuffers::FlexbufferSerializer::new();
            block
                .header
                .serialize(&mut serializer)
                .expect("could not serialize header");
            Hash::hash(&serializer.take_buffer())
        };
        massa_trace!("start acknowledge new block",{"thread_number": block.header.thread_number,"thread_count":self.cfg.thread_count});
        // check if we already have the block
        if self.seen_blocks.contains(&header_hash) {
            massa_trace!("already seen", { "block": &block });
            return (false, header_hash);
        }

        if block.header.thread_number >= self.cfg.thread_count {
            massa_trace!("wrong thread number", {"block": &block,"thread_number": block.header.thread_number,"thread_count":self.cfg.thread_count});
            self.seen_blocks.insert(header_hash);
            return (false, header_hash);
        }
        if self.active_blocks[block.header.thread_number as usize].contains_key(&header_hash) {
            massa_trace!("already active", { "block": &block });
            return (false, header_hash);
        }

        // check block validity
        if !self.is_nongenesis_block_valid(&header_hash, &block, selector) {
            massa_trace!("non valid", { "block": &block });
            self.seen_blocks.insert(header_hash);
            return (false, header_hash);
        }

        // let block_hash = {
        //     let mut serializer = flexbuffers::FlexbufferSerializer::new();
        //     block
        //         .serialize(&mut serializer)
        //         .expect("could not serialize header");
        //     Hash::hash(&serializer.take_buffer())
        // };

        // update best parents
        if self.best_parents[block.header.thread_number as usize].0 < block.header.slot_number {
            self.best_parents[block.header.thread_number as usize] =
                (block.header.slot_number, header_hash);
        }

        self.active_blocks[block.header.thread_number as usize].insert(header_hash, block.clone());
        massa_trace!("acknowledged", { "block": &block });
        (true, header_hash)
    }
}
