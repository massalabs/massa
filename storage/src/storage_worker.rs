use crypto::hash::Hash;

use models::block::Block;
use sled::{Db, Tree};
use std::collections::HashMap;

use crate::{config::StorageConfig, error::StorageError};

#[derive(Clone)]
pub struct BlockStorage {
    hash_to_block: Tree,
    slot_to_hash: Tree,
    db: Db,
}

impl BlockStorage {
    pub fn open(cfg: &StorageConfig) -> Result<BlockStorage, StorageError> {
        let sled_config = sled::Config::default()
            .path(&cfg.path)
            .cache_capacity(cfg.cache_capacity)
            .flush_every_ms(cfg.flush_every_ms);
        let db = sled_config.open()?;
        let hash_to_block = db.open_tree("hash_to_block")?;
        let slot_to_hash = db.open_tree("slot_to_hash")?;
        Ok(BlockStorage {
            hash_to_block,
            slot_to_hash,
            db,
        })
    }

    pub fn add_block(&self, hash: Hash, block: Block) -> Result<(), StorageError> {
        todo!()
    }

    pub fn get_block(&self, hash: Hash) -> Result<Option<Block>, StorageError> {
        todo!()
    }

    pub fn get_slot_range(
        &self,
        start: (u64, u8),
        end: (u64, u8),
    ) -> Result<HashMap<Hash, Block>, StorageError> {
        todo!()
    }
}

#[derive(Debug, Clone)]
pub enum StorageEvent {}
