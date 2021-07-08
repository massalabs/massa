use crate::{config::StorageConfig, error::StorageError, storage_worker::BlockStorage};
use crypto::hash::Hash;
use logging::debug;
use models::{block::Block, slot::Slot};
use std::collections::HashMap;

pub fn start_storage_controller(
    cfg: StorageConfig,
) -> Result<(StorageCommandSender, StorageManager), StorageError> {
    debug!("starting storage controller");
    let db = BlockStorage::open(cfg)?;
    Ok((StorageCommandSender(db.clone()), StorageManager(db)))
}

#[derive(Clone)]
pub struct StorageCommandSender(pub BlockStorage);

impl StorageCommandSender {
    pub async fn clear(&self) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.clear()).await?
    }
    pub async fn len(&self) -> Result<usize, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.len()).await?
    }
    pub async fn add_block(&self, hash: Hash, block: Block) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.add_block(hash, block)).await?
    }
    pub async fn add_block_batch(
        &self,
        hash: Hash,
        blocks: HashMap<Hash, Block>,
    ) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.add_block_batch(blocks)).await?
    }

    pub async fn add_multiple_blocks(
        &self,
        blocks: HashMap<Hash, Block>,
    ) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.add_block_batch(blocks)).await?
    }

    pub async fn get_block(&self, hash: Hash) -> Result<Option<Block>, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.get_block(hash)).await?
    }

    pub async fn contains(&self, hash: Hash) -> Result<bool, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.contains(hash)).await?
    }

    pub async fn get_slot_range(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<HashMap<Hash, Block>, StorageError> {
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
