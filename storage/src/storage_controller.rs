use logging::{debug, massa_trace};
use std::collections::VecDeque;

use tokio::{sync::mpsc, task::JoinHandle};

use crate::{
    config::{StorageConfig, CHANNEL_SIZE},
    error::StorageError,
    storage_worker::{StorageCommand, StorageEvent, StorageManagementCommand, StorageWorker},
};

pub async fn start_storage_controller(
    cfg: StorageConfig,
) -> Result<(StorageCommandSender, StorageEventReceiver, StorageManager), StorageError> {
    debug!("starting storage controller");
    massa_trace!("start", {});
    let (command_tx, command_rx) = mpsc::channel::<StorageCommand>(CHANNEL_SIZE);
    let (event_tx, event_rx) = mpsc::channel::<StorageEvent>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<StorageManagementCommand>(1);
    let cfg_copy = cfg.clone();
    let join_handle = tokio::spawn(async move {
        StorageWorker::new(cfg_copy, command_rx, event_tx, manager_rx)?
            .run_loop()
            .await
    });
    Ok((
        StorageCommandSender(command_tx),
        StorageEventReceiver(event_rx),
        StorageManager {
            join_handle,
            manager_tx,
        },
    ))
}

#[derive(Clone)]
pub struct StorageCommandSender(pub mpsc::Sender<StorageCommand>);

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

pub struct StorageManager {
    join_handle: JoinHandle<Result<(), StorageError>>,
    manager_tx: mpsc::Sender<StorageManagementCommand>,
}

impl StorageManager {
    pub async fn stop() {
        todo!()
    }
}
