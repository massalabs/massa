use tokio::sync::mpsc;

use crate::{config::StorageConfig, error::StorageError};

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
    pub fn new() -> Result<StorageWorker, StorageError> {
        todo!()
    }

    pub async fn run_loop(mut self) {
        todo!()
    }
}
