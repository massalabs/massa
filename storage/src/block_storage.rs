use crate::{
    config::StorageConfig,
    error::{InternalError, StorageError},
};
use models::{Block, BlockId, DeserializeCompact, SerializationContext, SerializeCompact, Slot};
use sled::{self, Transactional};
use std::{
    collections::HashMap,
    convert::TryInto,
    sync::atomic::{AtomicUsize, Ordering},
    sync::Arc,
};
use tokio::sync::{mpsc, oneshot, Notify};

pub struct StorageCleaner {
    max_stored_blocks: usize,
    notify: Arc<Notify>,
    shutdown: Arc<Notify>,
    clear_rx: mpsc::Receiver<oneshot::Sender<()>>,
    block_count: Arc<AtomicUsize>,
    hash_to_block: sled::Tree,
    slot_to_hash: sled::Tree,
}

impl StorageCleaner {
    pub fn new(
        max_stored_blocks: usize,
        notify: Arc<Notify>,
        shutdown: Arc<Notify>,
        clear_rx: mpsc::Receiver<oneshot::Sender<()>>,
        hash_to_block: sled::Tree,
        slot_to_hash: sled::Tree,
        block_count: Arc<AtomicUsize>,
    ) -> Result<Self, StorageError> {
        Ok(StorageCleaner {
            max_stored_blocks,
            notify,
            shutdown,
            clear_rx,
            block_count,
            hash_to_block,
            slot_to_hash,
        })
    }

    pub async fn run_loop(mut self) -> Result<(), StorageError> {
        loop {
            massa_trace!("storage.storage_cleaner.run_loop.select", {});
            let shutdown = tokio::select! {
                _ = self.notify.notified() => {
                    massa_trace!("storage.storage_cleaner.run_loop.notified", {});
                    false
                },
                _ = self.shutdown.notified() => {
                    massa_trace!("storage.storage_cleaner.run_loop.shutdown", {});
                    true
                },
                Some(response_sender) = self.clear_rx.recv() => {
                    massa_trace!("storage.storage_cleaner.run_loop.clear", {});
                    let current_count = self.block_count.load(Ordering::Acquire);

                    // clear all key-value stores
                    self.hash_to_block.clear()?;
                    self.slot_to_hash.clear()?;

                    // Note: this is not completely accurate,
                    // since storage could have added blocks between the load and the clears.
                    self.block_count.fetch_sub(current_count, Ordering::Release);

                    response_sender.send(()).map_err(|_| {
                        StorageError::ClearError("Couldn't send the clear command response.".into())
                    })?;
                    continue;
                },
            };

            // 1. Always run the cleaner.
            let mut current_count = self.block_count.load(Ordering::Acquire);
            let mut removed = 0;
            while current_count > self.max_stored_blocks {
                if let Some((slot, hash)) = self.slot_to_hash.first()? {
                    (&self.slot_to_hash, &self.hash_to_block).transaction(|(slots, hashes)| {
                        // Note: below assertions are unlikely to panic,
                        // since only the cleaner removes keys from the trees.
                        slots
                            .remove(slot.clone())?
                            .expect("Slot should have been previously inserted");
                        hashes
                            .remove(hash.clone())?
                            .expect("Block should have been previously inserted");
                        Ok(())
                    })?;
                }
                current_count -= 1;
                removed += 1;
            }
            self.block_count.fetch_sub(removed, Ordering::Release);

            // 2. Maybe shutdown.
            if shutdown {
                return Ok(());
            }
        }
    }
}

#[derive(Clone)]
pub struct BlockStorage {
    cfg: StorageConfig,
    serialization_context: SerializationContext,
    block_count: Arc<AtomicUsize>,
    hash_to_block: sled::Tree,
    slot_to_hash: sled::Tree,
    notify: Arc<Notify>,
    clear_tx: mpsc::Sender<oneshot::Sender<()>>,
}

impl BlockStorage {
    pub async fn clear(&self) -> Result<(), StorageError> {
        // Note: do the clearing in the cleaner,
        // to ensure only the cleaner is removing stuff from the trees,
        // and the prevent the count from going back above zero
        // if the cleaner is in the middle of a clean.
        let (response_tx, response_rx) = oneshot::channel();
        self.clear_tx.send(response_tx).await.map_err(|_| {
            StorageError::ClearError("Couldn't send the clear command to the cleaner.".into())
        })?;
        response_rx.await.map_err(|_| {
            StorageError::ClearError("Couldn't receive the clear response from the cleaner.".into())
        })?;
        Ok(())
    }

    pub fn open(
        cfg: StorageConfig,
        serialization_context: SerializationContext,
        hash_to_block: sled::Tree,
        slot_to_hash: sled::Tree,
        block_count: Arc<AtomicUsize>,
        notify: Arc<Notify>,
        clear_tx: mpsc::Sender<oneshot::Sender<()>>,
    ) -> Result<BlockStorage, StorageError> {
        let res = BlockStorage {
            cfg,
            serialization_context,
            block_count,
            hash_to_block,
            slot_to_hash,
            notify,
            clear_tx,
        };

        return Ok(res);
    }

    pub async fn add_block(&self, block_id: BlockId, block: Block) -> Result<(), StorageError> {
        //acquire W lock on block_count
        massa_trace!("block_storage.add_block", {"block_id": block_id, "block": block});

        //add the new block
        if self.add_block_internal(block_id, block).await? {
            self.block_count.fetch_add(1, Ordering::Release);
        };
        self.notify.notify_one();

        Ok(())
    }

    pub async fn add_block_batch(
        &self,
        blocks: HashMap<BlockId, Block>,
    ) -> Result<(), StorageError> {
        let mut newly_added = 0;

        //add the new blocks
        for (block_id, block) in blocks.into_iter() {
            massa_trace!("block_storage.add_block_batch", {"block_id": block_id, "block": block});
            if self.add_block_internal(block_id, block).await? {
                newly_added += 1;
            };
        }

        self.block_count.fetch_add(newly_added, Ordering::Release);
        self.notify.notify_one();

        Ok(())
    }

    /// Returns a boolean indicating whether the block was a new addition(true) or a replacement(false).
    async fn add_block_internal(
        &self,
        block_id: BlockId,
        block: Block,
    ) -> Result<bool, StorageError> {
        //add the new block
        (&self.hash_to_block, &self.slot_to_hash)
            .transaction(|(hash_tx, slot_tx)| {
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
                if slot_tx
                    .insert(
                        &block.header.content.slot.to_bytes_key(),
                        &block_id.to_bytes(),
                    )?
                    .is_none()
                {
                    return Ok(true);
                };
                Ok(false)
            })
            .map_err(|err| StorageError::AddBlockError(format!("Error adding a block: {:?}", err)))
    }

    pub async fn len(&self) -> Result<usize, StorageError> {
        // Note: we're not synchronizing this across clearing/pruning,
        // so we might as well keep the ordering relaxed.
        Ok(self.block_count.load(Ordering::Relaxed))
    }

    pub async fn contains(&self, block_id: BlockId) -> Result<bool, StorageError> {
        self.hash_to_block
            .contains_key(block_id.to_bytes())
            .map_err(|e| StorageError::from(e))
    }

    pub fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, StorageError> {
        massa_trace!("block_storage.get_block", { "block_id": block_id });
        let hash_key = block_id.to_bytes();
        if let Some(s_block) = self.hash_to_block.get(hash_key)? {
            Ok(Some(
                Block::from_bytes_compact(s_block.as_ref(), &self.serialization_context)?.0,
            ))
        } else {
            Ok(None)
        }
    }

    pub async fn get_slot_range(
        &self,
        start: Option<Slot>,
        end: Option<Slot>,
    ) -> Result<HashMap<BlockId, Block>, StorageError> {
        let start_key = start.map(|v| v.to_bytes_key());
        let end_key = end.map(|v| v.to_bytes_key());

        match (start_key, end_key) {
            (None, None) => self.slot_to_hash.iter(),
            (Some(b1), None) => self.slot_to_hash.range(b1..),
            (None, Some(b2)) => self.slot_to_hash.range(..b2),
            (Some(b1), Some(b2)) => self.slot_to_hash.range(b1..b2),
        }
        .filter_map(|item| {
            let (_, s_hash) = item.ok()?;
            let hash = BlockId::from_bytes(
                &s_hash
                    .as_ref()
                    .try_into()
                    .map_err(|err| {
                        StorageError::DeserializationError(format!(
                            "wrong buffer size for hash deserialization: {:?}",
                            err
                        ))
                    })
                    .ok()?,
            )
            .ok()?;

            // Note: blocks not found are ignored.
            match self.get_block(hash) {
                Ok(Some(block)) => Some(Ok((hash, block))),
                _ => None,
            }
        })
        .collect::<Result<HashMap<BlockId, Block>, StorageError>>()
    }
}
