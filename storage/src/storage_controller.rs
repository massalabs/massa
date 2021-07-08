use crate::storage_worker::BlockStorage;
use crypto::hash::Hash;
use logging::{debug, massa_trace};
use models::block::Block;
use std::collections::HashMap;
use std::collections::VecDeque;
use tokio::sync::mpsc;

use crate::{
    config::{StorageConfig, CHANNEL_SIZE},
    error::StorageError,
    storage_worker::StorageEvent,
};

pub fn start_storage_controller(
    cfg: StorageConfig,
) -> Result<(StorageCommandSender, StorageEventReceiver, StorageManager), StorageError> {
    debug!("starting storage controller");
    massa_trace!("start", {});
    let (_event_tx, event_rx) = mpsc::channel::<StorageEvent>(CHANNEL_SIZE);
    let db = BlockStorage::open(&cfg)?;
    Ok((
        StorageCommandSender(db.clone()),
        StorageEventReceiver(event_rx),
        StorageManager(db),
    ))
}

#[derive(Clone)]
pub struct StorageCommandSender(pub BlockStorage);

impl StorageCommandSender {
    pub async fn add_block(&self, hash: Hash, block: Block) -> Result<(), StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.add_block(hash, block)).await?
    }

    pub async fn get_block(&self, hash: Hash) -> Result<Option<Block>, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.get_block(hash)).await?
    }

    pub async fn get_slot_range(
        &self,
        start: (u64, u8),
        end: (u64, u8),
    ) -> Result<HashMap<Hash, Block>, StorageError> {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || db.get_slot_range(start, end)).await?
    }
}

pub struct StorageEventReceiver(pub mpsc::Receiver<StorageEvent>);

impl StorageEventReceiver {
    pub async fn wait_event(&mut self) -> Result<StorageEvent, StorageError> {
        self.0
            .recv()
            .await
            .ok_or(StorageError::ControllerEventError)
    }

    pub async fn drain(mut self) -> VecDeque<StorageEvent> {
        let mut remaining_events: VecDeque<StorageEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

pub struct StorageManager(pub BlockStorage);

impl StorageManager {
    pub async fn stop(
        self,
        storage_event_receiver: StorageEventReceiver,
    ) -> Result<(), StorageError> {
        let _remaining_events = storage_event_receiver.drain().await;
        Ok(())
    }
}
