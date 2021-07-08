use crypto::hash::Hash;
use models::{block::Block, slot::Slot};
use sled::Transactional;
use sled::Tree;
use sled::{transaction::ConflictableTransactionError, Db};
use std::sync::Arc;
use std::sync::RwLock;
use std::{collections::HashMap, ops::Deref};

use crate::{
    config::StorageConfig,
    error::{InternalError, StorageError},
};

#[derive(Clone)]
pub struct BlockStorage {
    db: Db,
    nb_stored_blocks: Arc<RwLock<usize>>,
    max_stored_blocks: usize,
}

impl BlockStorage {
    pub fn reset(&self) -> Result<(), StorageError> {
        self.db.open_tree("hash_to_block")?.clear()?;
        self.db.open_tree("slot_to_hash")?.clear()?;
        Ok(())
    }

    pub fn open(cfg: &StorageConfig) -> Result<BlockStorage, StorageError> {
        let sled_config = sled::Config::default()
            .path(&cfg.path)
            .cache_capacity(cfg.cache_capacity)
            .flush_every_ms(cfg.flush_every_ms);
        let db = sled_config.open()?;
        let _hash_to_block = db.open_tree("hash_to_block")?;
        let _slot_to_hash = db.open_tree("slot_to_hash")?;
        let nb_blocks_in_db = db.len();
        Ok(BlockStorage {
            db,
            nb_stored_blocks: Arc::new(RwLock::new(nb_blocks_in_db)),
            max_stored_blocks: cfg.max_stored_blocks,
        })
    }

    pub fn add_block(&self, hash: Hash, block: Block) -> Result<(), StorageError> {
        let hash_to_block = self.db.open_tree("hash_to_block")?;
        let slot_to_hash = self.db.open_tree("slot_to_hash")?;

        //manage max block. If nb block > max block, remove the oldest block.
        self.nb_stored_blocks
            .read()
            .map(|nb_stored_blocks| {
                if *nb_stored_blocks >= self.max_stored_blocks {
                    true
                } else {
                    false
                }
            })
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))
            .and_then(|max_reach| {
                if max_reach {
                    slot_to_hash.pop_min().and_then(|res| {
                        res.map(|(_, min_hash)| hash_to_block.remove(min_hash))
                            .transpose()
                    })?;
                    Ok(())
                    //.map_err(|err| err.into())
                } else {
                    self.nb_stored_blocks
                        .write()
                        .map(|mut value| {
                            *value += 1;
                        })
                        .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))
                }
            })?;

        (&hash_to_block, &slot_to_hash).transaction(|(hash_tx, slot_tx)| {
            let block_vec = block.into_bytes().map_err(|err| {
                ConflictableTransactionError::Abort(InternalError::TransactionError(format!(
                    "error serializing block: {:?}",
                    err
                )))
            })?;
            hash_tx.insert(&hash.to_bytes(), block_vec.as_slice())?;
            slot_tx.insert(
                &Slot::new(block.header.period_number, block.header.thread_number).to_bytes(),
                &hash.to_bytes(),
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn contains(&self, hash: Hash) -> Result<bool, StorageError> {
        let hash_to_block = self.db.open_tree("hash_to_block")?;
        hash_to_block
            .contains_key(hash.to_bytes())
            .map_err(|e| StorageError::from(e))
    }
    pub fn get_block(&self, hash: Hash) -> Result<Option<Block>, StorageError> {
        let hash_to_block = self.db.open_tree("hash_to_block")?;
        BlockStorage::get_block_internal(hash, &hash_to_block)
    }

    fn get_block_internal(hash: Hash, hash_to_block: &Tree) -> Result<Option<Block>, StorageError> {
        hash_to_block
            .get(hash.to_bytes())?
            .map(|sblock| Block::from_bytes(sblock.deref().into()))
            .transpose()
            .map_err(|e| StorageError::from(e))
    }

    pub fn get_slot_range(
        &self,
        start: (u64, u8),
        end: (u64, u8),
    ) -> Result<HashMap<Hash, Block>, StorageError> {
        let hash_to_block = self.db.open_tree("hash_to_block")?;
        let slot_to_hash = self.db.open_tree("slot_to_hash")?;
        let start = Slot::from_tuple(start).to_bytes();
        let end = Slot::from_tuple(end).to_bytes();
        slot_to_hash
            .range(start..end)
            .map(|res| {
                res.map_err(|e| StorageError::from(e))
                    .and_then(|(_, shash)| {
                        let hash = Hash::from_bytes(shash.deref().into())?;
                        let block = BlockStorage::get_block_internal(hash, &hash_to_block)?;
                        Ok(block.map(|b| (hash, b)))
                    })
            })
            .filter_map(|val| val.transpose())
            .collect::<Result<HashMap<Hash, Block>, StorageError>>()
    }
}
