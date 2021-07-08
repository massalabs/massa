use crypto::hash::Hash;
use models::{block::Block, slot::Slot};
use sled::Transactional;
use sled::{transaction::ConflictableTransactionError, Db};
use tokio::sync::{mpsc, oneshot};

use std::{collections::HashMap, ops::Deref};

use crate::{
    config::StorageConfig,
    error::{InternalError, StorageError},
};

#[derive(Clone)]
pub struct BlockStorage {
    db: Db,
}

impl BlockStorage {
    pub fn open(cfg: &StorageConfig) -> Result<BlockStorage, StorageError> {
        let sled_config = sled::Config::default()
            .path(&cfg.path)
            .cache_capacity(cfg.cache_capacity)
            .flush_every_ms(cfg.flush_every_ms);
        let db = sled_config.open()?;
        let _hash_to_block = db.open_tree("hash_to_block")?;
        let _slot_to_hash = db.open_tree("slot_to_hash")?;
        Ok(BlockStorage { db })
    }

    pub fn add_block(&self, hash: Hash, block: Block) -> Result<(), StorageError> {
        let hash_to_block = self.db.open_tree("hash_to_block")?;
        let slot_to_hash = self.db.open_tree("slot_to_hash")?;
        (&hash_to_block, &slot_to_hash).transaction(|(hash_tx, slot_tx)| {
            let block_vec = match block.into_bytes() {
                Ok(b) => b,
                Err(e) => {
                    return Err(ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!("error serializing block")),
                    ))
                }
            };
            hash_tx.insert(&hash.to_bytes(), block_vec.as_slice())?;
            slot_tx.insert(
                Slot::new(block.header.period_number, block.header.thread_number).into_bytes(),
                &hash.to_bytes(),
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn get_block(&self, hash: Hash) -> Result<Option<Block>, StorageError> {
        let hash_to_block = self.db.open_tree("hash_to_block")?;
        hash_to_block
            .get(&hash.to_bytes())
            .map(|v| {
                if let Some(b) = v {
                    match Block::from_bytes(b.deref().into()) {
                        Ok(b) => Ok(Some(b)),
                        Err(e) => {
                            return Err(e);
                        }
                    }
                } else {
                    Ok(None)
                }
            })?
            .map_err(|e| StorageError::from(e))
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
