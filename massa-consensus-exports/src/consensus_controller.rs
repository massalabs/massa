//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_graph::{BlockGraphExport, BootstrapableGraph};
use massa_models::api::BlockGraphStatus;
use massa_models::{clique::Clique, stats::ConsensusStats};
use massa_models::{BlockId, Slot};
use massa_protocol_exports::ProtocolEventReceiver;
use massa_storage::Storage;
use std::collections::VecDeque;

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use crate::{
    commands::{ConsensusCommand, ConsensusManagementCommand},
    error::ConsensusResult as Result,
    events::ConsensusEvent,
    ConsensusError,
};

/// Consensus commands sender
/// TODO Make private
#[derive(Clone)]
pub struct ConsensusCommandSender(pub mpsc::Sender<ConsensusCommand>);

impl ConsensusCommandSender {
    /// Gets all the available information on the block graph returning a `BlockGraphExport`.
    ///
    /// # Arguments
    /// * `slot_start`: optional slot start for slot-based filtering (included).
    /// * `slot_end`: optional slot end for slot-based filtering (excluded).
    pub async fn get_block_graph_status(
        &self,
        slot_start: Option<Slot>,
        slot_end: Option<Slot>,
    ) -> Result<BlockGraphExport> {
        let (response_tx, response_rx) = oneshot::channel::<BlockGraphExport>();
        massa_trace!("consensus.consensus_controller.get_block_graph_status", {});
        self.0
            .send(ConsensusCommand::GetBlockGraphStatus {
                slot_start,
                slot_end,
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_block_graph_status".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_block_graph_status response read error".to_string(),
            )
        })
    }

    /// Gets all cliques.
    ///
    pub async fn get_cliques(&self) -> Result<Vec<Clique>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<Clique>>();
        massa_trace!("consensus.consensus_controller.get_cliques", {});
        self.0
            .send(ConsensusCommand::GetCliques(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_cliques".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_cliques response read error".to_string(),
            )
        })
    }

    /// Gets the graph statuses of a batch of blocks.
    ///
    /// # Arguments
    /// * ids: array of block IDs
    pub async fn get_block_statuses(
        &self,
        ids: &[BlockId],
    ) -> Result<Vec<BlockGraphStatus>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<BlockGraphStatus>>();
        massa_trace!("consensus.consensus_controller.get_block_statuses", {});
        self.0
            .send(ConsensusCommand::GetBlockStatuses {
                ids: ids.iter().cloned().collect(),
                response_tx,
            })
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_block_statuses".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_block_statuses response read error".to_string(),
            )
        })
    }

    /// get bootstrap snapshot
    pub async fn get_bootstrap_state(&self) -> Result<BootstrapableGraph, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<BootstrapableGraph>();
        massa_trace!("consensus.consensus_controller.get_bootstrap_state", {});
        self.0
            .send(ConsensusCommand::GetBootstrapState(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_bootstrap_state".into(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_bootstrap_state response read error".to_string(),
            )
        })
    }

    /// get best parents
    pub fn get_best_parents(&self) -> Result<Vec<(BlockId, u64)>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel::<Vec<(BlockId, u64)>>();
        massa_trace!("consensus.consensus_controller.get_best_parents", {});
        self.0
            .blocking_send(ConsensusCommand::GetBestParents { response_tx })
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_best_parents".into(),
                )
            })?;
        response_rx.blocking_recv().map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_best_parents response read error".to_string(),
            )
        })
    }

    /// get block id of a slot in a blockclique
    pub fn get_blockclique_block_at_slot(
        &self,
        slot: Slot,
    ) -> Result<Option<BlockId>, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!(
            "consensus.consensus_controller.get_blockclique_block_at_slot",
            { "slot": slot }
        );
        self.0
            .blocking_send(ConsensusCommand::GetBlockcliqueBlockAtSlot { slot, response_tx })
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_blockclique_block_at_slot".into(),
                )
            })?;
        response_rx.blocking_recv().map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_blockclique_block_at_slot response read error".to_string(),
            )
        })
    }

    /// get current consensus stats
    pub async fn get_stats(&self) -> Result<ConsensusStats, ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.get_stats", {});
        self.0
            .send(ConsensusCommand::GetStats(response_tx))
            .await
            .map_err(|_| {
                ConsensusError::SendChannelError(
                    "send error consensus command get_stats".to_string(),
                )
            })?;
        response_rx.await.map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command get_stats response read error".to_string(),
            )
        })
    }

    ///send block
    pub fn send_block(
        &self,
        block_id: BlockId,
        slot: Slot,
        block_storage: Storage,
    ) -> Result<(), ConsensusError> {
        let (response_tx, response_rx) = oneshot::channel();
        massa_trace!("consensus.consensus_controller.send_block", {
            "block_id": block_id
        });
        self.0
            .blocking_send(ConsensusCommand::SendBlock {
                block_id,
                slot,
                block_storage,
                response_tx,
            })
            .map_err(|_| {
                ConsensusError::SendChannelError("send error consensus command send_block".into())
            })?;
        response_rx.blocking_recv().map_err(|_| {
            ConsensusError::ReceiveChannelError(
                "consensus command send_block response read error".to_string(),
            )
        })
    }
}

/// channel to receive consensus events
pub struct ConsensusEventReceiver(pub mpsc::Receiver<ConsensusEvent>);

impl ConsensusEventReceiver {
    /// wait for the next event
    pub async fn wait_event(&mut self) -> Result<ConsensusEvent, ConsensusError> {
        self.0
            .recv()
            .await
            .ok_or(ConsensusError::ControllerEventError)
    }

    /// drains remaining events and returns them in a `VecDeque`
    /// note: events are sorted from oldest to newest
    pub async fn drain(mut self) -> VecDeque<ConsensusEvent> {
        let mut remaining_events: VecDeque<ConsensusEvent> = VecDeque::new();

        while let Some(evt) = self.0.recv().await {
            remaining_events.push_back(evt);
        }
        remaining_events
    }
}

/// Consensus manager
pub struct ConsensusManager {
    /// protocol handler
    pub join_handle: JoinHandle<Result<ProtocolEventReceiver, ConsensusError>>,
    /// consensus management sender
    pub manager_tx: mpsc::Sender<ConsensusManagementCommand>,
}

impl ConsensusManager {
    /// stop consensus
    pub async fn stop(
        self,
        consensus_event_receiver: ConsensusEventReceiver,
    ) -> Result<ProtocolEventReceiver, ConsensusError> {
        massa_trace!("consensus.consensus_controller.stop", {});
        drop(self.manager_tx);
        let _remaining_events = consensus_event_receiver.drain().await;
        let protocol_event_receiver = self.join_handle.await??;

        Ok(protocol_event_receiver)
    }
}
