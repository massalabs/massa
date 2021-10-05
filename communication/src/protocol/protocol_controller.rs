// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::{
    config::{ProtocolConfig, CHANNEL_SIZE},
    protocol_worker::{
        ProtocolCommand, ProtocolEvent, ProtocolManagementCommand, ProtocolPoolEvent,
        ProtocolWorker,
    },
};
use crate::{
    error::CommunicationError,
    network::{NetworkCommandSender, NetworkEventReceiver},
};
use models::{
    Block, BlockHashMap, BlockHashSet, BlockId, Endorsement, EndorsementHashMap, EndorsementId,
    Operation, OperationHashMap, OperationHashSet,
};
use std::collections::VecDeque;
use tokio::{sync::mpsc, task::JoinHandle};

/// start a new ProtocolController from a ProtocolConfig
/// - generate public / private key
/// - create protocol_command/protocol_event channels
/// - launch protocol_controller_fn in an other task
///
/// # Arguments
/// * cfg : protocol configuration
/// * network_command_sender: the NetworkCommandSender we interact with
/// * network_event_receiver: the NetworkEventReceiver we interact with
pub async fn start_protocol_controller(
    cfg: ProtocolConfig,
    operation_validity_periods: u64,
    network_command_sender: NetworkCommandSender,
    network_event_receiver: NetworkEventReceiver,
) -> Result<
    (
        ProtocolCommandSender,
        ProtocolEventReceiver,
        ProtocolPoolEventReceiver,
        ProtocolManager,
    ),
    CommunicationError,
> {
    debug!("starting protocol controller");

    // launch worker
    let (event_tx, event_rx) = mpsc::channel::<ProtocolEvent>(CHANNEL_SIZE);
    let (pool_event_tx, pool_event_rx) = mpsc::channel::<ProtocolPoolEvent>(CHANNEL_SIZE);
    let (command_tx, command_rx) = mpsc::channel::<ProtocolCommand>(CHANNEL_SIZE);
    let (manager_tx, manager_rx) = mpsc::channel::<ProtocolManagementCommand>(1);
    let join_handle = tokio::spawn(async move {
        let res = ProtocolWorker::new(
            cfg,
            operation_validity_periods,
            network_command_sender,
            network_event_receiver,
            event_tx,
            pool_event_tx,
            command_rx,
            manager_rx,
        )
        .run_loop()
        .await;
        match res {
            Err(err) => {
                error!("protocol worker crashed: {:?}", err);
                Err(err)
            }
            Ok(v) => {
                info!("protocol worker finished cleanly");
                Ok(v)
            }
        }
    });
    debug!("protocol controller ready");
    Ok((
        ProtocolCommandSender(command_tx),
        ProtocolEventReceiver(event_rx),
        ProtocolPoolEventReceiver(pool_event_rx),
        ProtocolManager {
            join_handle,
            manager_tx,
        },
    ))
}

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
        operation_ids: OperationHashSet,
        endorsement_ids: Vec<EndorsementId>,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.command_sender.integrated_block", { "block_id": block_id, "block": block });
        let res = self
            .0
            .send(ProtocolCommand::IntegratedBlock {
                block_id,
                block,
                operation_ids,
                endorsement_ids,
            })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("block_integrated command send error".into())
            });
        res
    }

    /// Notify to protocol an attack attempt.
    pub async fn notify_block_attack(
        &mut self,
        block_id: BlockId,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.command_sender.notify_block_attack", {
            "block_id": block_id
        });
        let res = self
            .0
            .send(ProtocolCommand::AttackBlockDetected(block_id))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("notify_block_attack command send error".into())
            });
        res
    }

    /// Send the response to a ProtocolEvent::GetBlocks.
    pub async fn send_get_blocks_results(
        &mut self,
        results: BlockHashMap<
            Option<(Block, Option<OperationHashSet>, Option<Vec<EndorsementId>>)>,
        >,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.command_sender.send_get_blocks_results", {
            "results": results
        });
        let res = self
            .0
            .send(ProtocolCommand::GetBlocksResults(results))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError(
                    "send_get_blocks_results command send error".into(),
                )
            });
        res
    }

    pub async fn send_wishlist_delta(
        &mut self,
        new: BlockHashSet,
        remove: BlockHashSet,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.command_sender.send_wishlist_delta", { "new": new, "remove": remove });
        let res = self
            .0
            .send(ProtocolCommand::WishlistDelta { new, remove })
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("send_wishlist_delta command send error".into())
            });
        res
    }

    pub async fn propagate_operations(
        &mut self,
        operations: OperationHashMap<Operation>,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.command_sender.propagate_operations", {
            "operations": operations
        });
        let res = self
            .0
            .send(ProtocolCommand::PropagateOperations(operations))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("propagate_operation command send error".into())
            });
        res
    }

    pub async fn propagate_endorsements(
        &mut self,
        endorsements: EndorsementHashMap<Endorsement>,
    ) -> Result<(), CommunicationError> {
        massa_trace!("protocol.command_sender.propagate_endorsements", {
            "endorsements": endorsements
        });
        let res = self
            .0
            .send(ProtocolCommand::PropagateEndorsements(endorsements))
            .await
            .map_err(|_| {
                CommunicationError::ChannelError("propagate_endorsements command send error".into())
            });
        res
    }
}

pub struct ProtocolEventReceiver(pub mpsc::Receiver<ProtocolEvent>);

impl ProtocolEventReceiver {
    /// Receives the next ProtocolEvent from connected Node.
    /// None is returned when all Sender halves have dropped,
    /// indicating that no further values can be sent on the channel
    pub async fn wait_event(&mut self) -> Result<ProtocolEvent, CommunicationError> {
        massa_trace!("protocol.event_receiver.wait_event", {});
        let res = self.0.recv().await.ok_or_else(|| {
            CommunicationError::ChannelError(
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
    pub async fn wait_event(&mut self) -> Result<ProtocolPoolEvent, CommunicationError> {
        massa_trace!("protocol.pool_event_receiver.wait_event", {});
        let res = self.0.recv().await.ok_or_else(|| {
            CommunicationError::ChannelError(
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
    join_handle: JoinHandle<Result<NetworkEventReceiver, CommunicationError>>,
    manager_tx: mpsc::Sender<ProtocolManagementCommand>,
}

impl ProtocolManager {
    /// Stop the protocol controller
    pub async fn stop(
        self,
        protocol_event_receiver: ProtocolEventReceiver,
        protocol_pool_event_receiver: ProtocolPoolEventReceiver,
    ) -> Result<NetworkEventReceiver, CommunicationError> {
        drop(self.manager_tx);
        let _remaining_events = protocol_event_receiver.drain().await;
        let _remaining_events = protocol_pool_event_receiver.drain().await;
        let network_event_receiver = self.join_handle.await??;
        Ok(network_event_receiver)
    }
}
