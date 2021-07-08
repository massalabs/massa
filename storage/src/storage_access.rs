use crate::{
    block_storage::{BlockStorage, StorageCleaner},
    config::StorageConfig,
    error::StorageError,
};
use logging::debug;
use models::{Block, BlockId, SerializationContext, Slot};
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot::Sender, Notify};
use tokio::task::JoinHandle;

pub fn start_storage(
    cfg: StorageConfig,
    serialization_context: SerializationContext,
) -> Result<(StorageAccess, StorageManager), StorageError> {
    debug!("starting storage controller");
    let sled_config = sled::Config::default()
        .path(&cfg.path)
        .cache_capacity(cfg.cache_capacity)
        .flush_every_ms(cfg.flush_interval.map(|v| v.to_millis()));
    let db = sled_config.open()?;

    if cfg.reset_at_startup {
        db.drop_tree("hash_to_block")?;
        db.drop_tree("slot_to_hash")?;
    }
    let hash_to_block = db.open_tree("hash_to_block")?;
    let slot_to_hash = db.open_tree("slot_to_hash")?;

    let block_count = Arc::new(AtomicUsize::new(slot_to_hash.len()));
    let notify = Arc::new(Notify::new());
    let (clear_tx, clear_rx) = mpsc::channel::<Sender<()>>(1);
    let db = BlockStorage::open(
        cfg.clone(),
        serialization_context,
        hash_to_block.clone(),
        slot_to_hash.clone(),
        block_count.clone(),
        notify.clone(),
        clear_tx,
    )?;

    let shutdown = Arc::new(Notify::new());
    let storage_cleaner = StorageCleaner::new(
        cfg.max_stored_blocks,
        notify.clone(),
        shutdown.clone(),
        clear_rx,
        hash_to_block,
        slot_to_hash,
        block_count,
    )?;
    let join_handle = tokio::spawn(async move {
        let res = storage_cleaner.run_loop().await;
        match res {
            Err(err) => {
                error!("Storage cleaner crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("Storage cleaner finished cleanly");
                Ok(v)
            }
        }
    });
    Ok((
        StorageAccess(db.clone()),
        StorageManager {
            shutdown,
            join_handle,
        },
    ))
}

#[derive(Clone)]
pub struct StorageAccess(pub BlockStorage);

impl StorageAccess {
    pub async fn clear(&self) -> Result<(), StorageError> {
        self.0.clear().await
    }
    pub async fn len(&self) -> Result<usize, StorageError> {
        self.0.len().await
    }
    pub async fn add_block(&self, hash: BlockId, block: Block) -> Result<(), StorageError> {
        self.0.add_block(hash, block).await
    }
    pub async fn add_block_batch(
        &self,
        blocks: HashMap<BlockId, Block>,
    ) -> Result<(), StorageError> {
        self.0.add_block_batch(blocks).await
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
    ) -> Result<HashMap<BlockId, Block>, StorageError> {
        self.0.get_slot_range(start, end).await
    }
}

pub struct StorageManager {
    shutdown: Arc<Notify>,
    join_handle: JoinHandle<Result<(), StorageError>>,
}

impl StorageManager {
    pub async fn stop(self) -> Result<(), StorageError> {
        self.shutdown.notify_one();
        self.join_handle.await?
    }
}
