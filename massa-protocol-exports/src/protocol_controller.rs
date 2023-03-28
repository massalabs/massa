// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ProtocolError;
use massa_logging::massa_trace;

use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::{
    block_header::SecuredHeader, block_id::BlockId, endorsement::EndorsementId,
    operation::OperationId,
};
use massa_network_exports::NetworkEventReceiver;
use massa_storage::Storage;
use serde::Serialize;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::info;

/// block result: map block id to
/// ```md
/// Option(
///     Option(set(operation id)),
///     Option(Vec(endorsement id))
/// )
/// ```
pub type BlocksResults =
    PreHashMap<BlockId, Option<(Option<PreHashSet<OperationId>>, Option<Vec<EndorsementId>>)>>;

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum ProtocolCommand {
    /// Notify block integration of a given block.
    IntegratedBlock {
        /// block id
        block_id: BlockId,
        /// block storage
        storage: Storage,
    },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(BlockId),
    /// Wish list delta
    WishlistDelta {
        /// add to wish list
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        /// remove from wish list
        remove: PreHashSet<BlockId>,
    },
    /// Propagate operations (send batches)
    /// note: `Set<OperationId>` are replaced with `OperationPrefixIds`
    ///       by the controller
    PropagateOperations(Storage),
    /// Propagate endorsements
    PropagateEndorsements(Storage),
}

/// protocol management commands
#[derive(Debug, Serialize)]
pub enum ProtocolManagementCommand {}

/// protocol command sender
#[derive(Clone)]
pub struct ProtocolCommandSender(pub mpsc::Sender<ProtocolCommand>);

impl ProtocolCommandSender {
    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * `block_id`: ID of the block
    /// * `storage`: Storage instance containing references to the block and all its dependencies
    pub fn integrated_block(
        &mut self,
        block_id: BlockId,
        storage: Storage,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.integrated_block", {
            "block_id": block_id
        });
        self.0
            .blocking_send(ProtocolCommand::IntegratedBlock { block_id, storage })
            .map_err(|_| ProtocolError::ChannelError("block_integrated command send error".into()))
    }

    /// Notify to protocol an attack attempt.
    pub fn notify_block_attack(&mut self, block_id: BlockId) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.notify_block_attack", {
            "block_id": block_id
        });
        self.0
            .blocking_send(ProtocolCommand::AttackBlockDetected(block_id))
            .map_err(|_| {
                ProtocolError::ChannelError("notify_block_attack command send error".into())
            })
    }

    /// update the block wish list
    pub fn send_wishlist_delta(
        &mut self,
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        remove: PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.send_wishlist_delta", { "new": new, "remove": remove });
        self.0
            .blocking_send(ProtocolCommand::WishlistDelta { new, remove })
            .map_err(|_| {
                ProtocolError::ChannelError("send_wishlist_delta command send error".into())
            })
    }

    /// Propagate a batch of operation ids (from pool).
    ///
    /// note: Full `OperationId` is replaced by a `OperationPrefixId` later by the worker.
    pub fn propagate_operations(&mut self, operations: Storage) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.propagate_operations", {
            "operations": operations.get_op_refs()
        });
        self.0
            .blocking_send(ProtocolCommand::PropagateOperations(operations))
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_operation command send error".into())
            })
    }

    /// propagate endorsements to connected node
    pub fn propagate_endorsements(&mut self, endorsements: Storage) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.propagate_endorsements", {
            "endorsements": endorsements.get_endorsement_refs()
        });
        self.0
            .blocking_send(ProtocolCommand::PropagateEndorsements(endorsements))
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_endorsements command send error".into())
            })
    }
}

/// protocol manager used to stop the protocol
pub struct ProtocolManager {
    join_handle: JoinHandle<Result<NetworkEventReceiver, ProtocolError>>,
    manager_tx: mpsc::Sender<ProtocolManagementCommand>,
}

impl ProtocolManager {
    /// new protocol manager
    pub fn new(
        join_handle: JoinHandle<Result<NetworkEventReceiver, ProtocolError>>,
        manager_tx: mpsc::Sender<ProtocolManagementCommand>,
    ) -> Self {
        ProtocolManager {
            join_handle,
            manager_tx,
        }
    }

    /// Stop the protocol controller
    pub async fn stop(self) -> Result<NetworkEventReceiver, ProtocolError> {
        info!("stopping protocol controller...");
        drop(self.manager_tx);
        let network_event_receiver = self.join_handle.await??;
        info!("protocol controller stopped");
        Ok(network_event_receiver)
    }
}
