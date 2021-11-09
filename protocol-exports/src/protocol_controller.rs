// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::error::ProtocolError;
use async_trait::async_trait;
use logging::massa_trace;
use network::NetworkEventReceiver;

use models::{
    Block, BlockHashMap, BlockHashSet, BlockHeader, BlockId, Endorsement, EndorsementHashMap,
    EndorsementId, Operation, OperationHashMap, OperationHashSet,
};
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
        operation_set: OperationHashMap<(usize, u64)>, // (index, validity end period)
        endorsement_ids: Vec<EndorsementId>,
    },
    /// A block header with a valid signature has been received.
    ReceivedBlockHeader {
        block_id: BlockId,
        header: BlockHeader,
    },
    /// Ask for a list of blocks from consensus.
    GetBlocks(Vec<BlockId>),
}
/// Possible types of pool events that can happen.
#[derive(Debug, Serialize)]
pub enum ProtocolPoolEvent {
    /// Operations were received
    ReceivedOperations {
        operations: OperationHashMap<Operation>,
        propagate: bool, // whether or not to propagate operations
    },
    /// Endorsements were received
    ReceivedEndorsements {
        endorsements: EndorsementHashMap<Endorsement>,
        propagate: bool, // whether or not to propagate endorsements
    },
}

#[derive(Debug, Serialize)]
pub enum ProtocolManagementCommand {}

pub trait ProtocolInterfaceClone {
    fn clone_box(&self) -> Box<dyn ProtocolInterface>;
}

impl Clone for Box<dyn ProtocolInterface> {
    fn clone(&self) -> Box<dyn ProtocolInterface> {
        self.clone_box()
    }
}

#[async_trait]
pub trait ProtocolInterface: Send + Sync + ProtocolInterfaceClone {
    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * hash : hash of the block header
    async fn integrated_block(
        &mut self,
        block_id: BlockId,
        block: Block,
        operation_ids: OperationHashSet,
        endorsement_ids: Vec<EndorsementId>,
    ) -> Result<(), ProtocolError>;

    /// Notify to protocol an attack attempt.
    async fn notify_block_attack(&mut self, block_id: BlockId) -> Result<(), ProtocolError>;

    /// Send the response to a ProtocolEvent::GetBlocks.
    async fn send_get_blocks_results(
        &mut self,
        results: BlockHashMap<
            Option<(Block, Option<OperationHashSet>, Option<Vec<EndorsementId>>)>,
        >,
    ) -> Result<(), ProtocolError>;

    async fn send_wishlist_delta(
        &mut self,
        new: BlockHashSet,
        remove: BlockHashSet,
    ) -> Result<(), ProtocolError>;

    async fn propagate_operations(
        &mut self,
        operations: OperationHashMap<Operation>,
    ) -> Result<(), ProtocolError>;

    async fn propagate_endorsements(
        &mut self,
        endorsements: EndorsementHashMap<Endorsement>,
    ) -> Result<(), ProtocolError>;
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
