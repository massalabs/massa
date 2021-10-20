// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{
    block_storage::{BlockStorage, StorageCleaner},
    config::StorageConfig,
    error::StorageError,
};
use models::{
    Address, Block, BlockHashMap, BlockHashSet, BlockId, OperationHashMap, OperationHashSet,
    OperationSearchResult, Slot,
};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

pub fn start_storage(cfg: StorageConfig) -> Result<(StorageAccess, StorageManager), StorageError> {
    debug!("starting storage controller");
    let sled_config = sled::Config::default()
        .path(&cfg.path)
        .cache_capacity(cfg.cache_capacity)
        .flush_every_ms(cfg.flush_interval.map(|v| v.to_millis()));
    let db = sled_config.open()?;

    if cfg.reset_at_startup {
        db.drop_tree("hash_to_block")?;
        db.drop_tree("slot_to_hash")?;
        db.drop_tree("op_to_block")?;
        db.drop_tree("addr_to_op")?;
        db.drop_tree("addr_to_block")?;
    }
    let hash_to_block = db.open_tree("hash_to_block")?;
    let slot_to_hash = db.open_tree("slot_to_hash")?;
    let op_to_block = db.open_tree("op_to_block")?;
    let addr_to_op = db.open_tree("addr_to_op")?;
    let addr_to_block = db.open_tree("addr_to_block")?;

    let block_count = Arc::new(AtomicUsize::new(hash_to_block.len()));
    let notify = Arc::new(Notify::new());
    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>(1);
    let db = BlockStorage::open(
        hash_to_block.clone(),
        slot_to_hash.clone(),
        op_to_block.clone(),
        addr_to_op.clone(),
        addr_to_block.clone(),
        block_count.clone(),
        notify.clone(),
        cfg.clone(),
    )?;

    let storage_cleaner = StorageCleaner::new(
        cfg.max_stored_blocks,
        notify,
        shutdown_rx,
        hash_to_block,
        slot_to_hash,
        op_to_block,
        addr_to_op,
        addr_to_block,
        block_count,
    )?;
    let join_handle = tokio::spawn(async move {
        let res = storage_cleaner.run_loop().await;
        match res {
            Err(err) => {
                error!("Storage cleaner crashed: {}", err);
                Err(err)
            }
            Ok(v) => {
                info!("Storage cleaner finished cleanly");
                Ok(v)
            }
        }
    });
    Ok((
        StorageAccess(db),
        StorageManager {
            shutdown_tx,
            join_handle,
        },
    ))
}

#[derive(Clone)]
pub struct StorageAccess(pub BlockStorage);

impl StorageAccess {
    pub async fn len(&self) -> Result<usize, StorageError> {
        self.0.len().await
    }
    pub async fn add_block(
        &self,
        hash: BlockId,
        block: Block,
        operation_set: OperationHashMap<(usize, u64)>,
    ) -> Result<bool, StorageError> {
        self.0.add_block(hash, block, operation_set).await
    }

    pub async fn get_operations_involving_address(
        &self,
        address: &Address,
    ) -> Result<OperationHashMap<OperationSearchResult>, StorageError> {
        self.0.get_operations_involving_address(address).await
    }

    pub async fn get_block(&self, hash: BlockId) -> Result<Option<Block>, StorageError> {
        self.0.get_block(hash)
    }

    pub async fn contains(&self, hash: BlockId) -> Result<bool, StorageError> {
        self.0.contains(hash).await
    }

    pub async fn get_slot_range(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<BlockHashMap<Block>, StorageError> {
        self.0.get_slot_range(start, end).await
    }

    /// returns Some(tuple) if found, or None if not found. Tuple:
    ///  * the BlockId in which the op is included
    ///  * its index in the block
    ///  * the operation itself
    pub async fn get_operations(
        &self,
        operation_ids: OperationHashSet,
    ) -> Result<OperationHashMap<OperationSearchResult>, StorageError> {
        self.0.get_operations(operation_ids).await
    }

    pub async fn get_block_ids_by_creator(
        &self,
        address: &Address,
    ) -> Result<BlockHashSet, StorageError> {
        self.0.get_block_ids_by_creator(address).await
    }
}

pub struct StorageManager {
    shutdown_tx: mpsc::Sender<()>,
    join_handle: JoinHandle<Result<(), StorageError>>,
}

impl StorageManager {
    pub async fn stop(self) -> Result<(), StorageError> {
        drop(self.shutdown_tx);
        self.join_handle.await?
    }
}
