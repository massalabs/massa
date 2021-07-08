use std::collections::VecDeque;

use tokio::sync::mpsc;

use crate::{
    error::StorageError,
    storage_worker::{StorageCommand, StorageEvent},
};

pub async fn start_storage_controller() {
    todo!();
}

#[derive(Clone)]
pub struct StorageCommandSender(pub mpsc::Sender<StorageCommand>);

pub struct StorageEventReceiver(pub mpsc::Receiver<StorageEvent>);

impl StorageEventReceiver {
    pub async fn wait_event(&mut self) -> Result<StorageEvent, StorageError> {
        todo!()
    }

    pub async fn drain(mut self) -> VecDeque<StorageEvent> {
        todo!()
    }
}

pub struct StorageManager {}

impl StorageManager {
    pub async fn stop() {
        todo!()
    }
}
