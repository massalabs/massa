use crate::{
    config::StorageConfig,
    error::{InternalError, StorageError},
};
use models::{Block, BlockId, DeserializeCompact, SerializationContext, SerializeCompact, Slot};
use sled::{self, Transactional};
use std::{
    collections::HashMap,
    convert::TryInto,
    sync::{Arc, RwLock},
};

#[derive(Clone)]
pub struct BlockStorage {
    cfg: StorageConfig,
    serialization_context: SerializationContext,
    db: sled::Db,
    block_count: Arc<RwLock<usize>>,
    hash_to_block: sled::Tree,
    slot_to_hash: sled::Tree,
}

impl BlockStorage {
    pub fn clear(&self) -> Result<(), StorageError> {
        //acquire W lock on block_count
        let mut block_count_w = self
            .block_count
            .write()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;

        // clear all key-value stores
        self.hash_to_block.clear()?;
        self.slot_to_hash.clear()?;
        *block_count_w = 0;

        Ok(())
    }

    pub fn open(
        cfg: StorageConfig,
        serialization_context: SerializationContext,
    ) -> Result<BlockStorage, StorageError> {
        let sled_config = sled::Config::default()
            .path(&cfg.path)
            .cache_capacity(cfg.cache_capacity)
            .flush_every_ms(cfg.flush_interval.map(|v| v.to_millis()));
        let db = sled_config.open()?;
        let hash_to_block = db.open_tree("hash_to_block")?;
        let slot_to_hash = db.open_tree("slot_to_hash")?;
        let block_count = Arc::new(RwLock::new(db.len()));

        let res = BlockStorage {
            cfg,
            serialization_context,
            db,
            block_count: block_count.clone(),
            hash_to_block,
            slot_to_hash,
        };

        //ensure max block count. while nb block > max block, remove the oldest blocks.
        {
            let mut block_count_w = block_count
                .write()
                .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;
            res.remove_excess_blocks(&mut block_count_w)?;
        }

        Ok(res)
    }

    pub fn add_block(&self, block_id: BlockId, block: Block) -> Result<(), StorageError> {
        //acquire W lock on block_count
        massa_trace!("block_storage.add_block", {"block_id": block_id, "block": block});
        let mut block_count_w = self
            .block_count
            .write()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;

        //add the new block
        self.add_block_internal(block_id, block, &mut block_count_w)?;

        Ok(())
    }

    pub fn add_block_batch(&self, blocks: HashMap<BlockId, Block>) -> Result<(), StorageError> {
        //acquire W lock on block_count
        let mut block_count_w = self
            .block_count
            .write()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;

        //add the new blocks
        for (block_id, block) in blocks.into_iter() {
            massa_trace!("block_storage.add_block_batch", {"block_id": block_id, "block": block});
            self.add_block_internal(block_id, block, &mut block_count_w)?;
        }

        Ok(())
    }

    fn add_block_internal(
        &self,
        block_id: BlockId,
        block: Block,
        block_count_w: &mut usize,
    ) -> Result<(), StorageError> {
        //add the new block
        (&self.hash_to_block, &self.slot_to_hash).transaction(|(hash_tx, slot_tx)| {
            let serialized_block = block
                .to_bytes_compact(&self.serialization_context)
                .map_err(|err| {
                    sled::transaction::ConflictableTransactionError::Abort(
                        InternalError::TransactionError(format!(
                            "error serializing block: {:?}",
                            err
                        )),
                    )
                })?;
            hash_tx.insert(&block_id.to_bytes(), serialized_block.as_slice())?;
            slot_tx.insert(
                &block.header.content.slot.to_bytes_key(),
                &block_id.to_bytes(),
            )?;
            Ok(())
        })?;
        *block_count_w += 1;

        //manage max block. If nb block > max block, remove the oldest block.
        self.remove_excess_blocks(block_count_w)?;

        Ok(())
    }

    /// while there are too many blocks, remove the one with the oldest slot
    fn remove_excess_blocks(&self, block_count_w: &mut usize) -> Result<(), StorageError> {
        while *block_count_w > self.cfg.max_stored_blocks {
            let (_block, hash) =
                self.slot_to_hash
                    .pop_min()?
                    .ok_or(StorageError::DatabaseInconsistency(
                        "block_count > 0 but slot_to_hash.pop_min returned None".into(),
                    ))?;
            self.hash_to_block.remove(hash)?;
            *block_count_w -= 1;
        }
        Ok(())
    }

    pub fn len(&self) -> Result<usize, StorageError> {
        self.block_count
            .read()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))
            .map(|nb_stored_blocks| *nb_stored_blocks)
    }

    pub fn contains(&self, block_id: BlockId) -> Result<bool, StorageError> {
        let _block_count_r = self
            .block_count
            .read()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;
        self.hash_to_block
            .contains_key(block_id.to_bytes())
            .map_err(StorageError::from)
    }

    pub fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, StorageError> {
        massa_trace!("block_storage.get_block", { "block_id": block_id });
        let hash_key = block_id.to_bytes();

        let _block_count_r = self
            .block_count
            .read()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;

        if let Some(s_block) = self.hash_to_block.get(hash_key)? {
            Ok(Some(
                Block::from_bytes_compact(s_block.as_ref(), &self.serialization_context)?.0,
            ))
        } else {
            Ok(None)
        }
    }

    pub fn get_slot_range(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<HashMap<BlockId, Block>, StorageError> {
        let start_key = start.map(|v| v.to_bytes_key());
        let end_key = end.map(|v| v.to_bytes_key());

        let _block_count_r = self
            .block_count
            .read()
            .map_err(|err| StorageError::MutexPoisonedError(err.to_string()))?;

        match (start_key, end_key) {
            (None, None) => self.slot_to_hash.iter(),
            (Some(b1), None) => self.slot_to_hash.range(b1..),
            (None, Some(b2)) => self.slot_to_hash.range(..b2),
            (Some(b1), Some(b2)) => self.slot_to_hash.range(b1..b2),
        }
        .map(|item| {
            let (_, s_hash) = item?;
            let hash = BlockId::from_bytes(&s_hash.as_ref().try_into().map_err(|err| {
                StorageError::DeserializationError(format!(
                    "wrong buffer size for hash deserialization: {:?}",
                    err
                ))
            })?)?;
            let block = self
                .get_block(hash)?
                .ok_or(StorageError::DatabaseInconsistency(
                    "block hash referenced by slot_to_hash is absent from hash_to_block".into(),
                ))?;
            Ok((hash, block))
        })
        .collect::<Result<HashMap<BlockId, Block>, StorageError>>()
    }
}
