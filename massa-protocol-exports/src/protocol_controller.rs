// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ProtocolError;
use massa_logging::massa_trace;

use massa_models::prehash::{Map, Set};

use massa_models::{
    Block, BlockId, EndorsementId, OperationId, SignedEndorsement, SignedHeader, SignedOperation,
};
use massa_network::NetworkEventReceiver;
use serde::Serialize;
use std::collections::VecDeque;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::debug;

/// Possible types of events that can happen.
#[derive(Debug, Serialize)]
pub enum ProtocolEvent {
    /// A block with a valid signature has been received.
    ReceivedBlock {
        block_id: BlockId,
        block: Block,
        operation_set: Map<OperationId, (usize, u64)>, // (index, validity end period)
        endorsement_ids: Map<EndorsementId, u32>,
    },
    /// A block header with a valid signature has been received.
    ReceivedBlockHeader {
        block_id: BlockId,
        header: SignedHeader,
    },
    /// Ask for a list of blocks from consensus.
    GetBlocks(Vec<BlockId>),
}
/// Possible types of pool events that can happen.
#[derive(Debug, Serialize)]
pub enum ProtocolPoolEvent {
    /// Operations were received
    ReceivedOperations {
        operations: Map<OperationId, SignedOperation>,
        propagate: bool, // whether or not to propagate operations
    },
    /// Endorsements were received
    ReceivedEndorsements {
        endorsements: Map<EndorsementId, SignedEndorsement>,
        propagate: bool, // whether or not to propagate endorsements
    },
}

type BlocksResults =
    Map<BlockId, Option<(Block, Option<Set<OperationId>>, Option<Vec<EndorsementId>>)>>;

/// Commands that protocol worker can process
#[derive(Debug, Serialize)]
pub enum ProtocolCommand {
    /// Notify block integration of a given block.
    IntegratedBlock {
        block_id: BlockId,
        block: Box<Block>,
        operation_ids: Set<OperationId>,
        endorsement_ids: Vec<EndorsementId>,
    },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(BlockId),
    /// Wishlist delta
    WishlistDelta {
        new: Set<BlockId>,
        remove: Set<BlockId>,
    },
    /// The response to a ProtocolEvent::GetBlocks.
    GetBlocksResults(BlocksResults),
    /// Propagate operations
    PropagateOperations(Map<OperationId, SignedOperation>),
    /// Propagate endorsements
    PropagateEndorsements(Map<EndorsementId, SignedEndorsement>),
}

#[derive(Debug, Serialize)]
pub enum ProtocolManagementCommand {}

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
        block: Block,
        operation_ids: Set<OperationId>,
        endorsement_ids: Vec<EndorsementId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.integrated_block", { "block_id": block_id, "block": block });
        let res = self
            .0
            .send(ProtocolCommand::IntegratedBlock {
                block_id,
                block: Box::new(block),
                operation_ids,
                endorsement_ids,
            })
            .await
            .map_err(|_| ProtocolError::ChannelError("block_integrated command send error".into()));
        res
    }

    /// Notify to protocol an attack attempt.
    pub async fn notify_block_attack(&mut self, block_id: BlockId) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.notify_block_attack", {
            "block_id": block_id
        });
        let res = self
            .0
            .send(ProtocolCommand::AttackBlockDetected(block_id))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("notify_block_attack command send error".into())
            });
        res
    }

    /// Send the response to a ProtocolEvent::GetBlocks.
    pub async fn send_get_blocks_results(
        &mut self,
        results: BlocksResults,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.send_get_blocks_results", {
            "results": results
        });
        let res = self
            .0
            .send(ProtocolCommand::GetBlocksResults(results))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("send_get_blocks_results command send error".into())
            });
        res
    }

    pub async fn send_wishlist_delta(
        &mut self,
        new: Set<BlockId>,
        remove: Set<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.send_wishlist_delta", { "new": new, "remove": remove });
        let res = self
            .0
            .send(ProtocolCommand::WishlistDelta { new, remove })
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("send_wishlist_delta command send error".into())
            });
        res
    }

    pub async fn propagate_operations(
        &mut self,
        operations: Map<OperationId, SignedOperation>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.propagate_operations", {
            "operations": operations
        });
        let res = self
            .0
            .send(ProtocolCommand::PropagateOperations(operations))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_operation command send error".into())
            });
        res
    }

    pub async fn propagate_endorsements(
        &mut self,
        endorsements: Map<EndorsementId, SignedEndorsement>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.command_sender.propagate_endorsements", {
            "endorsements": endorsements
        });
        let res = self
            .0
            .send(ProtocolCommand::PropagateEndorsements(endorsements))
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_endorsements command send error".into())
            });
        res
    }
}

pub struct ProtocolEventReceiver(pub mpsc::Receiver<ProtocolEvent>);

impl ProtocolEventReceiver {
    /// Receives the next ProtocolEvent from connected Node.
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> Result<ProtocolEvent, ProtocolError> {
        massa_trace!("protocol.event_receiver.wait_event", {});
        let res = self.0.recv().await.ok_or_else(|| {
            ProtocolError::ChannelError(
                "DefaultProtocolController wait_event channel recv failed".into(),
            )
        });
        res
    }

    /// drains remaining events and returns them in a VecDeque
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
pub struct ProtocolPoolEventReceiver(pub mpsc::Receiver<ProtocolPoolEvent>);

impl ProtocolPoolEventReceiver {
    /// Receives the next ProtocolPoolEvent
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> Result<ProtocolPoolEvent, ProtocolError> {
        massa_trace!("protocol.pool_event_receiver.wait_event", {});
        let res = self.0.recv().await.ok_or_else(|| {
            ProtocolError::ChannelError(
                "DefaultProtocolController wait_pool_event channel recv failed".into(),
            )
        });
        res
    }

    /// drains remaining events and returns them in a VecDeque
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

pub struct ProtocolManager {
    join_handle: JoinHandle<Result<NetworkEventReceiver, ProtocolError>>,
    manager_tx: mpsc::Sender<ProtocolManagementCommand>,
}

impl ProtocolManager {
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
