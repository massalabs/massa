use sled::{Db, Tree};
use tokio::sync::mpsc;

use crate::{config::StorageConfig, error::StorageError};

struct BlockStorage {
    hash_to_block: Tree,
    slot_to_hash: Tree,
    db: Db,
}

impl BlockStorage {
    pub fn open(cfg: StorageConfig) -> Result<BlockStorage, StorageError> {
        let sled_config = sled::Config::default()
            .path(cfg.path)
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
}
#[derive(Debug)]
pub enum StorageCommand {}

#[derive(Debug, Clone)]
pub enum StorageEvent {}

#[derive(Debug, Clone)]
pub enum StorageManagementCommand {}

pub struct StorageWorker {
    cfg: StorageConfig,
    controller_command_rx: mpsc::Receiver<StorageCommand>,
    controller_event_tx: mpsc::Sender<StorageEvent>,
    controller_manager_rx: mpsc::Receiver<StorageManagementCommand>,
}

impl StorageWorker {
    pub fn new(
        cfg: StorageConfig,
        controller_command_rx: mpsc::Receiver<StorageCommand>,
        controller_event_tx: mpsc::Sender<StorageEvent>,
        controller_manager_rx: mpsc::Receiver<StorageManagementCommand>,
    ) -> Result<StorageWorker, StorageError> {
        Ok(StorageWorker {
            cfg,
            controller_command_rx,
            controller_event_tx,
            controller_manager_rx,
        })
    }

    pub async fn run_loop(mut self) -> Result<(), StorageError> {
        loop {
            tokio::select! {
                Some(cmd) = self.controller_command_rx.recv() => match cmd {
                    _=> {},
                },

                cmd = self.controller_manager_rx.recv() => match cmd {
                    None => break,
                    Some(_) => {}
                }
            }
        }
        Ok(())
    }
}
