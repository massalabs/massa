use massa_storage::Storage;

use massa_versioning_exports::{
    VersioningCommand, VersioningConfig, VersioningError, VersioningManagementCommand,
    VersioningManager, VersioningReceivers, VersioningSenders,
};

use crate::versioning_controller::VersioningMiddleware;

use tokio::sync::mpsc;
use tracing::{debug, error, info};

/// # Arguments
/// * `config`: versioning settings
/// * `senders`: sender(s) channel(s) to communicate with other modules
/// * `receivers`: receiver(s) channel(s) to communicate with other modules
/// * `storage`: Shared storage to fetch data that are fetch across all modules
pub async fn start_versioning_worker(
    config: VersioningConfig,
    receivers: VersioningReceivers,
    _senders: VersioningSenders,
    storage: Storage,
) -> Result<VersioningManager, VersioningError> {
    // launch worker
    let (manager_tx, controller_manager_rx) = mpsc::channel::<VersioningManagementCommand>(1);
    let _join_handle = tokio::spawn(async move {
        let res = VersioningWorker::new(
            config,
            VersioningWorkerChannels {
                controller_command_rx: receivers.versioning_command_receiver,
                controller_manager_rx,
            },
            storage,
        )
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("protocol worker crashed: {}", err);
                Err(err)
            }
            Ok(v) => {
                info!("protocol worker finished cleanly");
                Ok(v)
            }
        }
    });
    debug!("protocol controller ready");
    Ok(VersioningManager::new(manager_tx))
}

/// versioning worker
pub struct VersioningWorker {
    /// Versioning configuration.
    pub(crate) config: VersioningConfig,
    /// Channel receiving commands from the controller.
    controller_command_rx: mpsc::Receiver<VersioningCommand>,
    /// Channel to send management commands to the controller.
    controller_manager_rx: mpsc::Receiver<VersioningManagementCommand>,
    /// Shared storage.
    pub(crate) storage: Storage,
    /// Versioning Middleware
    versioning_middleware: VersioningMiddleware,
}

/// channels used by the versioning worker
pub struct VersioningWorkerChannels {
    /// versioning command receiver
    pub controller_command_rx: mpsc::Receiver<VersioningCommand>,
    /// versioning management command receiver
    pub controller_manager_rx: mpsc::Receiver<VersioningManagementCommand>,
}

impl VersioningWorker {
    pub fn new(
        config: VersioningConfig,
        VersioningWorkerChannels {
            controller_command_rx,
            controller_manager_rx,
        }: VersioningWorkerChannels,
        storage: Storage,
    ) -> VersioningWorker {
        let versioning_middleware =
            VersioningMiddleware::new(config.nb_blocks_considered, config.threshold);

        VersioningWorker {
            config,
            controller_command_rx,
            controller_manager_rx,
            storage,
            versioning_middleware,
        }
    }

    pub async fn run_loop(mut self) -> Result<(), VersioningError> {
        loop {
            tokio::select! {
                cmd = self.controller_manager_rx.recv() => {
                    match cmd {
                        None => break,
                        Some(_) => {}
                    };
                }

                // listen to incoming commands
                Some(cmd) = self.controller_command_rx.recv() => {
                    self.process_command(cmd).await?;
                }
            }
        }

        Ok(())
    }

    async fn process_command(&mut self, cmd: VersioningCommand) -> Result<(), VersioningError> {
        match cmd {
            VersioningCommand::FinalizedBlockVersion { announced_version } => {
                self.versioning_middleware.new_block(announced_version);
            }
        }
        Ok(())
    }
}
