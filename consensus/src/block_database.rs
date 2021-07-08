use crate::error::ConsensusError;

use super::{config::ConsensusConfig, random_selector::RandomSelector};

use crypto::{hash::Hash, signature::SignatureEngine};
use models::block::Block;
use models::block::BlockHeader;

use std::collections::{HashMap, HashSet};

pub struct BlockDatabase {
    cfg: ConsensusConfig,
    pub active_blocks: Vec<HashMap<Hash, Block>>,
    pub seen_blocks: HashSet<Hash>,
    best_parents: Vec<(u64, Hash)>,
}

fn create_genesis_block(
    cfg: &ConsensusConfig,
    thread_number: u8,
) -> Result<(Hash, Block), ConsensusError> {
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
    let header_hash = header.compute_hash()?;

    let signature = signature_engine.sign(&header_hash, &private_key)?;
    Ok((
        header_hash,
        Block {
            header,
            signature,
            operations: Vec::new(),
        },
    ))
}

impl BlockDatabase {
    pub fn new(cfg: &ConsensusConfig) -> Result<Self, ConsensusError> {
        let mut active_blocks = vec![HashMap::new(); cfg.thread_count as usize];
        let mut seen_blocks = HashSet::new();
        let mut best_parents: Vec<(u64, Hash)> = Vec::with_capacity(cfg.thread_count as usize);
        for thread in 0u8..cfg.thread_count {
            let (genesis_block_hash, genesis_block) = create_genesis_block(cfg, thread)
                .map_err(|_err| ConsensusError::GenesisCreationError)?;
            active_blocks[thread as usize].insert(genesis_block_hash, genesis_block);
            seen_blocks.insert(genesis_block_hash);
            best_parents.push((0, genesis_block_hash));
        }
        Ok(BlockDatabase {
            cfg: cfg.clone(),
            active_blocks,
            seen_blocks,
            best_parents,
        })
    }

    pub fn create_block(
        &self,
        val: String,
        thread_number: u8,
        slot_number: u64,
    ) -> Result<Block, ConsensusError> {
        let signature_engine = SignatureEngine::new();
        let (public_key, private_key) = self
            .cfg
            .nodes
            .get(self.cfg.current_node_index as usize)
            .and_then(|(public_key, private_key)| Some((public_key.clone(), private_key.clone())))
            .ok_or(ConsensusError::KeyError)?;

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

        let header_hash = header.compute_hash()?;

        Ok(Block {
            header,
            operations: Vec::new(),
            signature: signature_engine.sign(&header_hash, &private_key)?,
        })
    }

    pub fn is_nongenesis_block_valid(
        &self,
        header_hash: &Hash,
        block: &Block,
        selector: &mut RandomSelector,
    ) -> Result<bool, ConsensusError> {
        // check values
        if block.header.parents.len() != (self.cfg.thread_count as usize) {
            return Ok(false);
        }
        if block.header.slot_number == 0 || block.header.thread_number >= self.cfg.thread_count {
            return Ok(false);
        }
        // TODO if block.header.endorsements.len() > xxx

        // check if it was their turn
        if block.header.roll_number
            != selector.draw(block.header.thread_number, block.header.slot_number)
        {
            return Ok(false);
        }

        // check signature
        let sig = SignatureEngine::new();
        Ok(sig.verify(&header_hash, &block.signature, &block.header.creator)?)
    }

    // - Check if block is new
    // - Check if block is valid
    // if yes insert block in blocks and returns true
    pub fn acknowledge_new_block(
        &mut self,
        block: &Block,
        selector: &mut RandomSelector,
    ) -> Result<(bool, Hash), ConsensusError> {
        // compute header hash
        let header_hash = block.header.compute_hash()?;

        massa_trace!("start acknowledge new block",{"thread_number": block.header.thread_number,"thread_count":self.cfg.thread_count});
        // check if we already have the block
        if self.seen_blocks.contains(&header_hash) {
            massa_trace!("already seen", { "block": &block });
            return Ok((false, header_hash));
        }

        if block.header.thread_number >= self.cfg.thread_count {
            massa_trace!("wrong thread number", {"block": &block,"thread_number": block.header.thread_number,"thread_count":self.cfg.thread_count});
            self.seen_blocks.insert(header_hash);
            return Ok((false, header_hash));
        }
        if self.active_blocks[block.header.thread_number as usize].contains_key(&header_hash) {
            massa_trace!("already active", { "block": &block });
            return Ok((false, header_hash));
        }

        // check block validity
        if !self.is_nongenesis_block_valid(&header_hash, &block, selector)? {
            massa_trace!("non valid", { "block": &block });
            self.seen_blocks.insert(header_hash);
            return Ok((false, header_hash));
        }

        // update best parents
        if self.best_parents[block.header.thread_number as usize].0 < block.header.slot_number {
            self.best_parents[block.header.thread_number as usize] =
                (block.header.slot_number, header_hash);
        }

        self.active_blocks[block.header.thread_number as usize].insert(header_hash, block.clone());
        massa_trace!("acknowledged", { "block": &block });
        Ok((true, header_hash))
    }
}
