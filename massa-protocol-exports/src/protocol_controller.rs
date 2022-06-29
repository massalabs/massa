// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ProtocolError;
use massa_logging::massa_trace;

use massa_models::{
    operation::OperationIds,
    prehash::{Map, Set},
    Slot, WrappedBlock,
};
use massa_models::{
    BlockId, EndorsementId, OperationId, WrappedEndorsement, WrappedHeader, WrappedOperation,
};
use massa_network_exports::NetworkEventReceiver;
use serde::Serialize;
use std::collections::VecDeque;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::debug;

/// Possible types of events that can happen.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize)]
pub enum ProtocolEvent {
    /// A block with a valid signature has been received.
    ReceivedBlock {
        /// corresponding block
        block: WrappedBlock,
        /// the slot
        slot: Slot,
        /// operations in the block by (index, validity end period)
        operation_set: Map<OperationId, (usize, u64)>,
        /// endorsements in the block with index
        endorsement_ids: Map<EndorsementId, u32>,
    },
    /// A block header with a valid signature has been received.
    ReceivedBlockHeader {
        /// its id
        block_id: BlockId,
        /// The header
        header: WrappedHeader,
    },
    /// Ask for a list of blocks from consensus.
    GetBlocks(Vec<BlockId>),
}
/// Possible types of pool events that can happen.
#[derive(Debug, Serialize)]
pub enum ProtocolPoolEvent {
    /// Operations were received
    ReceivedOperations {
        /// the operations
        operations: Map<OperationId, WrappedOperation>,
        /// whether or not to propagate operations
        propagate: bool,
    },
    /// Endorsements were received
    ReceivedEndorsements {
        /// the endorsements
        endorsements: Map<EndorsementId, WrappedEndorsement>,
        /// whether or not to propagate endorsements
        propagate: bool,
    },
}

/// block result: map block id to
/// ```md
/// Option(
///     Option(set(operation id)),
///     Option(Vec(endorsement id))
/// )
/// ```
pub type BlocksResults =
    Map<BlockId, Option<(Option<Set<OperationId>>, Option<Vec<EndorsementId>>)>>;

/// Commands that protocol worker can process
#[derive(Debug, Serialize)]
pub enum ProtocolCommand {
    /// Notify block integration of a given block.
    IntegratedBlock {
        /// block id
        block_id: BlockId,
        /// operations ids in the block
        operation_ids: OperationIds,
        /// endorsement ids in the block
        endorsement_ids: Vec<EndorsementId>,
    },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(BlockId),
    /// Wish list delta
    WishlistDelta {
        /// add to wish list
        new: Set<BlockId>,
        /// remove from wish list
        remove: Set<BlockId>,
    },
    /// The response to a `[ProtocolEvent::GetBlocks]`.
    GetBlocksResults(BlocksResults),
    /// Propagate operations (send batches)
    /// note: OperationIds are replaced with OperationPrefixIds
    ///       by the controller
    PropagateOperations(OperationIds),
    /// Propagate endorsements
    PropagateEndorsements(Map<EndorsementId, WrappedEndorsement>),
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
    /// * hash : hash of the block header
    pub async fn integrated_block(
        &mut self,
        block_id: BlockId,
        operation_ids: Set<OperationId>,
        endorsement_ids: Vec<EndorsementId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.integrated_block", {
            "block_id": block_id
        });
        self.0
            .send(ProtocolCommand::IntegratedBlock {
                block_id,
                operation_ids,
                endorsement_ids,
            })
            .await
            .map_err(|_| ProtocolError::ChannelError("block_integrated command send error".into()))
    }

    /// Notify to protocol an attack attempt.
    pub async fn notify_block_attack(&mut self, block_id: BlockId) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.notify_block_attack", {
            "block_id": block_id
        });
        self.0
            .send(ProtocolCommand::AttackBlockDetected(block_id))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("notify_block_attack command send error".into())
            })
    }

    /// Send the response to a `ProtocolEvent::GetBlocks`.
    pub async fn send_get_blocks_results(
        &mut self,
        results: BlocksResults,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.send_get_blocks_results", {
            "results": results
        });
        self.0
            .send(ProtocolCommand::GetBlocksResults(results))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("send_get_blocks_results command send error".into())
            })
    }

    /// update the block wish list
    pub async fn send_wishlist_delta(
        &mut self,
        new: Set<BlockId>,
        remove: Set<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.send_wishlist_delta", { "new": new, "remove": remove });
        self.0
            .send(ProtocolCommand::WishlistDelta { new, remove })
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("send_wishlist_delta command send error".into())
            })
    }

    /// Propagate a batch of operation ids (from pool).
    ///
    /// note: Full `OperationId` is replaced by a `OperationPrefixId` later by the worker.
    pub async fn propagate_operations(
        &mut self,
        operation_ids: OperationIds,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.propagate_operations", {
            "operations": operation_ids
        });
        self.0
            .send(ProtocolCommand::PropagateOperations(operation_ids))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_operation command send error".into())
            })
    }

    /// propagate endorsements to connected node
    pub async fn propagate_endorsements(
        &mut self,
        endorsements: Map<EndorsementId, WrappedEndorsement>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.propagate_endorsements", {
            "endorsements": endorsements
        });
        self.0
            .send(ProtocolCommand::PropagateEndorsements(endorsements))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_endorsements command send error".into())
            })
    }
}

/// Protocol event receiver
pub struct ProtocolEventReceiver(pub mpsc::Receiver<ProtocolEvent>);

impl ProtocolEventReceiver {
    /// Receives the next `ProtocolEvent` from connected Node.
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> Result<ProtocolEvent, ProtocolError> {
        massa_trace!("protocol.event_receiver.wait_event", {});
        self.0.recv().await.ok_or_else(|| {
            ProtocolError::ChannelError(
                "DefaultProtocolController wait_event channel recv failed".into(),
            )
        })
    }

    /// drains remaining events and returns them in a `VecDeque`
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ProtocolEvent> {
        let mut remaining_events: VecDeque<ProtocolEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            debug!(
                "after receiving event from ProtocolEventReceiver.0 in protocol_controller drain"
            );
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

/// Protocol pool event receiver
pub struct ProtocolPoolEventReceiver(pub mpsc::Receiver<ProtocolPoolEvent>);

impl ProtocolPoolEventReceiver {
    /// Receives the next `ProtocolPoolEvent`
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> Result<ProtocolPoolEvent, ProtocolError> {
        massa_trace!("protocol.pool_event_receiver.wait_event", {});
        self.0.recv().await.ok_or_else(|| {
            ProtocolError::ChannelError(
                "DefaultProtocolController wait_pool_event channel recv failed".into(),
            )
        })
    }

    /// drains remaining events and returns them in a `VecDeque`
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ProtocolPoolEvent> {
        let mut remaining_events: VecDeque<ProtocolPoolEvent> = VecDeque::new();
        while let Some(evt) = self.0.recv().await {
            debug!(
                "after receiving event from ProtocolPoolEventReceiver.0 in protocol_controller drain"
            );
            remaining_events.push_back(evt);
        }
        remaining_events
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
    pub async fn stop(
        self,
        protocol_event_receiver: ProtocolEventReceiver,
        protocol_pool_event_receiver: ProtocolPoolEventReceiver,
    ) -> Result<NetworkEventReceiver, ProtocolError> {
        drop(self.manager_tx);
        let _remaining_events = protocol_event_receiver.drain().await;
        let _remaining_events = protocol_pool_event_receiver.drain().await;
        let network_event_receiver = self.join_handle.await??;
        Ok(network_event_receiver)
    }
}
