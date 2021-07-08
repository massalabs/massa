use crate::{block_storage::BlockStorage, config::StorageConfig, error::StorageError};
use logging::debug;
use models::{Block, BlockId, SerializationContext, Slot};
use std::collections::HashMap;

pub fn start_storage(
    cfg: StorageConfig,
    serialization_context: SerializationContext,
) -> Result<(StorageAccess, StorageManager), StorageError> {
    debug!("starting storage controller");
    let db = BlockStorage::open(cfg.clone(), serialization_context)?;
    if cfg.reset_at_startup {
        db.clear()?;
    }
    Ok((StorageAccess(db.clone()), StorageManager(db)))
}

#[derive(Clone)]
pub struct StorageAccess(pub BlockStorage);

impl StorageAccess {
    pub async fn clear(&self) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.clear()).await?
    }
    pub async fn len(&self) -> Result<usize, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.len()).await?
    }
    pub async fn add_block(&self, hash: BlockId, block: Block) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.add_block(hash, block)).await?
    }
    pub async fn add_block_batch(
        &self,
        blocks: HashMap<BlockId, Block>,
    ) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.add_block_batch(blocks)).await?
    }

    pub async fn get_block(&self, hash: BlockId) -> Result<Option<Block>, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.get_block(hash)).await?
    }

    pub async fn contains(&self, hash: BlockId) -> Result<bool, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.contains(hash)).await?
    }

    pub async fn get_slot_range(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<HashMap<BlockId, Block>, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.get_slot_range(start, end)).await?
    }
}

pub struct StorageManager(pub BlockStorage);

impl StorageManager {
    pub async fn stop(self) -> Result<(), StorageError> {
        Ok(())
    }
}
