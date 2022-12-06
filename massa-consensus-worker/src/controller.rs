use jsonrpsee::{core::error::SubscriptionClosed, SubscriptionSink};
use massa_consensus_exports::{
    block_graph_export::BlockGraphExport, block_status::BlockStatus,
    bootstrapable_graph::BootstrapableGraph, error::ConsensusError,
    export_active_block::ExportActiveBlock, ConsensusController,
};
use massa_models::{
    api::BlockGraphStatus,
    block::{BlockHeader, BlockId, FilledBlock},
    clique::Clique,
    operation::{Operation, OperationId},
    prehash::PreHashSet,
    slot::Slot,
    stats::ConsensusStats,
    streaming_step::StreamingStep,
    wrapped::Wrapped,
};
use massa_storage::Storage;
use parking_lot::RwLock;
use serde::Serialize;
use std::sync::{mpsc::SyncSender, Arc};
use tokio::sync::broadcast::Sender;
use tokio_stream::wrappers::BroadcastStream;
use tracing::log::warn;

use crate::{commands::ConsensusCommand, state::ConsensusState, worker::WsConfig};

/// The retrieval of data is made using a shared state and modifications are asked by sending message to a channel.
/// This is done mostly to be able to:
///
/// - send commands through the channel without waiting for them to be processed from the point of view of the sending thread, and channels are very much optimal for that (much faster than locks)
/// - still be able to read the current state of the graph as processed so far (for this we need a shared state)
///
/// Note that sending commands and reading the state is done from different, mutually-asynchronous tasks and they can have data that are not sync yet.
#[derive(Clone)]
pub struct ConsensusControllerImpl {
    command_sender: SyncSender<ConsensusCommand>,
    ws_config: WsConfig,
    shared_state: Arc<RwLock<ConsensusState>>,
    bootstrap_part_size: u64,
}

impl ConsensusControllerImpl {
    pub fn new(
        command_sender: SyncSender<ConsensusCommand>,
        ws_config: WsConfig,
        shared_state: Arc<RwLock<ConsensusState>>,
        bootstrap_part_size: u64,
    ) -> Self {
        Self {
            command_sender,
            ws_config,
            shared_state,
            bootstrap_part_size,
        }
    }
}

impl ConsensusController for ConsensusControllerImpl {
    /// Get a block graph export in a given period.
    ///
    /// # Arguments:
    /// * `start_slot`: the start slot
    /// * `end_slot`: the end slot
    ///
    /// # Returns:
    /// An export of the block graph in this period
    fn get_block_graph_status(
        &self,
        start_slot: Option<Slot>,
        end_slot: Option<Slot>,
    ) -> Result<BlockGraphExport, ConsensusError> {
        self.shared_state
            .read()
            .extract_block_graph_part(start_slot, end_slot)
    }

    /// Get statuses of blocks present in the graph
    ///
    /// # Arguments:
    /// * `block_ids`: the block ids to get the status of
    ///
    /// # Returns:
    /// A vector of statuses sorted by the order of the block ids
    fn get_block_statuses(&self, ids: &[BlockId]) -> Vec<BlockGraphStatus> {
        let read_shared_state = self.shared_state.read();
        ids.iter()
            .map(|id| read_shared_state.get_block_status(id))
            .collect()
    }

    /// Get all the cliques possible in the block graph.
    ///
    /// # Returns:
    /// A vector of cliques
    fn get_cliques(&self) -> Vec<Clique> {
        self.shared_state.read().max_cliques.clone()
    }

    /// Get a part of the graph to send to a node so that he can setup his graph.
    /// Used for bootstrap.
    ///
    /// # Arguments:
    /// * `cursor`: streaming cursor containing the current state of bootstrap and what blocks have been to the client already
    /// * `execution_cursor`: streaming cursor of the final state to ensure that last slot of the bootstrap info corresponds
    ///
    /// # Returns:
    /// * A portion of the graph
    /// * The list of outdated block ids
    /// * The streaming step value after the current iteration
    fn get_bootstrap_part(
        &self,
        mut cursor: StreamingStep<PreHashSet<BlockId>>,
        execution_cursor: StreamingStep<Slot>,
    ) -> Result<
        (
            BootstrapableGraph,
            PreHashSet<BlockId>,
            StreamingStep<PreHashSet<BlockId>>,
        ),
        ConsensusError,
    > {
        let mut final_blocks: Vec<ExportActiveBlock> = Vec::new();
        let mut retrieved_ids: PreHashSet<BlockId> = PreHashSet::default();
        let read_shared_state = self.shared_state.read();
        let required_blocks: PreHashSet<BlockId> = match execution_cursor {
            StreamingStep::Ongoing(slot) | StreamingStep::Finished(Some(slot)) => {
                read_shared_state.list_required_active_blocks(Some(slot))?
            }
            _ => PreHashSet::default(),
        };

        let (current_ids, previous_ids, outdated_ids) = match cursor {
            StreamingStep::Started => (
                required_blocks,
                PreHashSet::default(),
                PreHashSet::default(),
            ),
            StreamingStep::Ongoing(ref cursor_ids) => (
                // ids that are contained in required_blocks but not in the download cursor => current_ids
                required_blocks.difference(cursor_ids).cloned().collect(),
                // ids previously downloaded => previous_ids
                cursor_ids.clone(),
                // ids previously downloaded but not contained in required_blocks anymore => outdated_ids
                cursor_ids.difference(&required_blocks).cloned().collect(),
            ),
            StreamingStep::Finished(_) => {
                return Ok((
                    BootstrapableGraph { final_blocks },
                    PreHashSet::default(),
                    cursor,
                ))
            }
        };

        for b_id in &current_ids {
            if let Some(BlockStatus::Active { a_block, storage }) =
                read_shared_state.block_statuses.get(b_id)
            {
                if final_blocks.len() as u64 >= self.bootstrap_part_size {
                    break;
                }
                match execution_cursor {
                    StreamingStep::Ongoing(slot) | StreamingStep::Finished(Some(slot)) => {
                        if a_block.slot > slot {
                            continue;
                        }
                    }
                    _ => (),
                }
                if a_block.is_final {
                    let export = ExportActiveBlock::from_active_block(a_block, storage);
                    final_blocks.push(export);
                    retrieved_ids.insert(*b_id);
                }
            }
        }

        if final_blocks.is_empty() {
            cursor = StreamingStep::Finished(None);
        } else {
            let pruned_previous_ids = previous_ids.difference(&outdated_ids);
            retrieved_ids.extend(pruned_previous_ids);
            cursor = StreamingStep::Ongoing(retrieved_ids);
        }

        Ok((BootstrapableGraph { final_blocks }, outdated_ids, cursor))
    }

    /// Get the stats of the consensus
    fn get_stats(&self) -> Result<ConsensusStats, ConsensusError> {
        self.shared_state.read().get_stats()
    }

    /// Get the current best parents for a block creation
    ///
    /// # Returns:
    /// A block id and a period for each thread of the graph
    fn get_best_parents(&self) -> Vec<(BlockId, u64)> {
        self.shared_state.read().best_parents.clone()
    }

    /// Get the block, that is in the blockclique, at a given slot.
    ///
    /// # Arguments:
    /// * `slot`: the slot to get the block at
    ///
    /// # Returns:
    /// The block id of the block at the given slot if exists
    fn get_blockclique_block_at_slot(&self, slot: Slot) -> Option<BlockId> {
        self.shared_state
            .read()
            .get_blockclique_block_at_slot(&slot)
    }

    /// Get the latest block, that is in the blockclique, in the thread of the given slot and before this `slot`.
    ///
    /// # Arguments:
    /// * `slot`: the slot that will give us the thread and the upper bound
    ///
    /// # Returns:
    /// The block id of the latest block in the thread of the given slot and before this slot
    fn get_latest_blockclique_block_at_slot(&self, slot: Slot) -> BlockId {
        self.shared_state
            .read()
            .get_latest_blockclique_block_at_slot(&slot)
    }

    fn register_block(&self, block_id: BlockId, slot: Slot, block_storage: Storage, created: bool) {
        block_storage.read_blocks().get(&block_id).map(|value| {
            let operations: Vec<(OperationId, Option<Wrapped<Operation, OperationId>>)> = value
                .content
                .operations
                .iter()
                .map(|operation_id| {
                    match block_storage.read_operations().get(operation_id).cloned() {
                        Some(wrapped_operation) => (*operation_id, Some(wrapped_operation)),
                        None => (*operation_id, None),
                    }
                })
                .collect();

            let _ = self.ws_config.block_sender.send(value.content.clone());
            self.ws_config.filled_block_sender.send(FilledBlock {
                header: value.content.header.clone(),
                operations,
            })
        });

        if let Err(err) = self
            .command_sender
            .try_send(ConsensusCommand::RegisterBlock(
                block_id,
                slot,
                block_storage,
                created,
            ))
        {
            warn!("error trying to register a block: {}", err);
        }
    }

    fn register_block_header(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        let _ = self
            .ws_config
            .block_header_sender
            .send(header.clone().content);
        if let Err(err) = self
            .command_sender
            .try_send(ConsensusCommand::RegisterBlockHeader(block_id, header))
        {
            warn!("error trying to register a block header: {}", err);
        }
    }

    fn mark_invalid_block(&self, block_id: BlockId, header: Wrapped<BlockHeader, BlockId>) {
        if let Err(err) = self
            .command_sender
            .try_send(ConsensusCommand::MarkInvalidBlock(block_id, header))
        {
            warn!("error trying to mark block as invalid: {}", err);
        }
    }

    fn clone_box(&self) -> Box<dyn ConsensusController> {
        Box::new(self.clone())
    }

    fn subscribe_new_blocks_headers(&self, sink: SubscriptionSink) {
        pipe(self.ws_config.block_header_sender.clone(), sink);
    }

    fn subscribe_new_blocks(&self, sink: SubscriptionSink) {
        pipe(self.ws_config.block_sender.clone(), sink);
    }

    fn subscribe_new_filled_blocks(&self, sink: SubscriptionSink) {
        pipe(self.ws_config.filled_block_sender.clone(), sink);
    }
}

fn pipe<T: Serialize + Send + Clone + 'static>(sender: Sender<T>, mut sink: SubscriptionSink) {
    let rx = BroadcastStream::new(sender.subscribe());
    tokio::spawn(async move {
        match sink.pipe_from_try_stream(rx).await {
            SubscriptionClosed::Success => {
                sink.close(SubscriptionClosed::Success);
            }
            SubscriptionClosed::RemotePeerAborted => (),
            SubscriptionClosed::Failed(err) => {
                sink.close(err);
            }
        };
    });
}
