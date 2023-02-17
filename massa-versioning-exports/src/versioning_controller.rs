// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::VersioningError;

use massa_models::block_header::BlockHeader;
use serde::Serialize;

use tokio::sync::mpsc;

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum VersioningCommand {
    /// Notify new finalized block header
    FinalizedBlockHeader { header: BlockHeader },
}

/// protocol management commands
#[derive(Debug, Serialize)]
pub enum VersioningManagementCommand {}

/// protocol command sender
#[derive(Clone)]
pub struct VersioningCommandSender(pub mpsc::Sender<VersioningCommand>);

impl VersioningCommandSender {}

/// versioning manager used to stop the protocol
pub struct VersioningManager {
    manager_tx: mpsc::Sender<VersioningManagementCommand>,
}

impl VersioningManager {
    /// new protocol manager
    pub fn new(manager_tx: mpsc::Sender<VersioningManagementCommand>) -> Self {
        VersioningManager { manager_tx }
    }

    /// Stop the protocol controller
    pub async fn stop(self) -> Result<(), VersioningError> {
        drop(self.manager_tx);

        Ok(())
    }
}
