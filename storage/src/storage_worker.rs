use std::collections::HashMap;

use crypto::hash::Hash;
use models::block::Block;
use sled::{Db, Tree};
use tokio::sync::{mpsc, oneshot};

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

    fn add_block(&self, hash: Hash, block: Block) -> Result<(), StorageError> {
        todo!()
    }

    fn get_block(&self, hash: Hash) -> Result<Option<Block>, StorageError> {
        todo!()
    }

    fn get_slot_range(
        &self,
        start: (u64, u8),
        end: (u64, u8),
    ) -> Result<HashMap<Hash, Block>, StorageError> {
        todo!()
    }
}
#[derive(Debug)]
pub enum StorageCommand {
    AddBlock(Hash, Block, oneshot::Sender<()>),
    GetBlock(Hash, oneshot::Sender<Option<Block>>),
    GetSlotRange((u64, u8), (u64, u8), oneshot::Sender<HashMap<Hash, Block>>),
}

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
                    StorageCommand::AddBlock(hash, block, reponse_tx) => {},
                    StorageCommand::GetBlock(hash, reponse_tx) => {},
                    StorageCommand::GetSlotRange(start, end, reponse_tx) => {},
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
