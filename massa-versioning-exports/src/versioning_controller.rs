// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::VersioningMiddlewareError;

use serde::Serialize;

use tokio::sync::mpsc;

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum VersioningCommand {
    /// Notify new finalized block header
    FinalizedBlockVersion { announced_version: u32 },
}

/// protocol management commands
#[derive(Debug, Serialize)]
pub enum VersioningManagementCommand {}

/// protocol command sender
#[derive(Clone)]
pub struct VersioningCommandSender(pub mpsc::Sender<VersioningCommand>);

impl VersioningCommandSender {
    pub fn send_block_version(
        &mut self,
        announced_version: u32,
    ) -> Result<(), VersioningMiddlewareError> {
        self.0
            .blocking_send(VersioningCommand::FinalizedBlockVersion { announced_version })
            .map_err(|_| {
                VersioningMiddlewareError::ChannelError(
                    "send_block_header command send error".into(),
                )
            })
    }
}

/// versioning manager used to stop the protocol
pub struct VersioningManager {
    manager_tx: mpsc::Sender<VersioningManagementCommand>,
}

impl VersioningManager {
    /// new versioning manager
    pub fn new(manager_tx: mpsc::Sender<VersioningManagementCommand>) -> Self {
        VersioningManager { manager_tx }
    }

    /// Stop the versioning controller
    pub fn stop(self) {
        drop(self.manager_tx);
    }
}
