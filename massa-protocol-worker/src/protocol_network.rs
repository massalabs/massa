//! Worker implementation for the network events
//!
//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::node_info::NodeInfo;
use crate::protocol_worker::ProtocolWorker;
use massa_hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    node::NodeId,
    prehash::{BuildMap, Map, Set},
    wrapped::{Id, Wrapped},
    BlockId, BlockSerializer, OperationId, WrappedOperation,
};
use massa_models::{Block, WrappedBlock};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NetworkEvent, ReplyForBlocksInfo};
use massa_protocol_exports::{ProtocolError, ProtocolEvent};
use massa_serialization::Serializer;
use tokio::time::{Instant, Sleep};
use tracing::{info, warn};

// static tracing messages
static NEW_CONN: &str = "protocol.protocol_worker.on_network_event.new_connection";
static CONN_CLOSED: &str = "protocol.protocol_worker.on_network_event.connection_closed";
static ASKED_BLOCKS: &str = "protocol.protocol_worker.on_network_event.asked_for_blocks";
static BLOCK_HEADER: &str = "protocol.protocol_worker.on_network_event.received_block_header";
static BLOCKS_INFO: &str = "protocol.protocol_worker.on_network_event.received_blocks_info";
static OPS: &str = "protocol.protocol_worker.on_network_event.received_operations";
static ENDORSEMENTS: &str = "protocol.protocol_worker.on_network_event.received_endorsements";
static OPS_BATCH: &str =
    "protocol.protocol_worker.on_network_event.received_operation_announcements";
static ASKED_OPS: &str = "protocol.protocol_worker.on_network_event.receive_ask_for_operations";

impl ProtocolWorker {
    /// Manages network event
    /// Only used by the worker.
    ///
    /// # Argument
    /// `evt`: event to process
    /// `block_ask_timer`: Timer to update to the next time we are able to ask a block
    pub(crate) async fn on_network_event(
        &mut self,
        evt: NetworkEvent,
        block_ask_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        match evt {
            NetworkEvent::NewConnection(node_id) => {
                info!("Connected to node {}", node_id);
                massa_trace!(NEW_CONN, { "node": node_id });
                self.active_nodes
                    .insert(node_id, NodeInfo::new(&self.config));
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::ConnectionClosed(node_id) => {
                massa_trace!(CONN_CLOSED, { "node": node_id });
                if self.active_nodes.remove(&node_id).is_some() {
                    // deletes all node info
                    info!("Connection closed with {}", node_id);
                    self.update_ask_block(block_ask_timer).await?;
                }
            }
            NetworkEvent::ReceivedBlockInfo {
                node: from_node_id,
                info,
            } => {
                massa_trace!(BLOCKS_INFO, { "node": from_node_id, "info": info });
                for (block_id, block_info) in info.into_iter() {
                    self.on_block_info_received(from_node_id, block_id, block_info)
                        .await?;
                }
                // Re-run the ask block algorithm.
                self.update_ask_block(block_ask_timer).await?;
            }
            NetworkEvent::AskedForBlocks {
                node: from_node_id,
                list,
            } => {
                massa_trace!(ASKED_BLOCKS, { "node": from_node_id, "hashlist": list});
                self.on_asked_for_blocks_received(from_node_id, list)
                    .await?;
            }
            NetworkEvent::ReceivedBlockHeader {
                source_node_id,
                header,
            } => {
                massa_trace!(BLOCK_HEADER, { "node": source_node_id, "header": header});
                if let Some((block_id, _endorsement_ids, is_new)) =
                    self.note_header_from_node(&header, &source_node_id).await?
                {
                    if is_new {
                        self.send_protocol_event(ProtocolEvent::ReceivedBlockHeader {
                            block_id,
                            header,
                        })
                        .await;
                    }
                    self.update_ask_block(block_ask_timer).await?;
                } else {
                    warn!(
                        "node {} sent us critically incorrect header, \
                        which may be an attack attempt by the remote node \
                        or a loss of sync between us and the remote node",
                        source_node_id,
                    );
                    let _ = self.ban_node(&source_node_id).await;
                }
            }
            NetworkEvent::ReceivedOperations { node, operations } => {
                massa_trace!(OPS, { "node": node, "operations": operations});
                self.on_operations_received(node, operations).await;
            }
            NetworkEvent::ReceivedEndorsements { node, endorsements } => {
                massa_trace!(ENDORSEMENTS, { "node": node, "endorsements": endorsements});
                if self
                    .note_endorsements_from_node(endorsements, &node, true)
                    .is_err()
                {
                    warn!(
                        "node {} sent us critically incorrect endorsements, \
                        which may be an attack attempt by the remote node or a \
                        loss of sync between us and the remote node",
                        node,
                    );
                    let _ = self.ban_node(&node).await;
                }
            }
            NetworkEvent::ReceivedOperationAnnouncements {
                node,
                operation_prefix_ids,
            } => {
                massa_trace!(OPS_BATCH, { "node": node, "operation_ids": operation_prefix_ids});
                self.on_operations_announcements_received(operation_prefix_ids, node)
                    .await?;
            }
            NetworkEvent::ReceiveAskForOperations {
                node,
                operation_prefix_ids,
            } => {
                massa_trace!(ASKED_OPS, { "node": node, "operation_ids": operation_prefix_ids});
                self.on_asked_operations_received(node, operation_prefix_ids)
                    .await?;
            }
        }
        Ok(())
    }

    /// Network ask the local node for blocks
    ///
    /// React on another node asking for blocks informations. We can forward the operation ids if
    /// the foreign node asked for `AskForBlocksInfo::Info` or the full operations if he asked for
    /// the missing operations in his storage with `AskForBlocksInfo::Operations`
    ///
    /// Forward the reply to the network.
    async fn on_asked_for_blocks_received(
        &mut self,
        from_node_id: NodeId,
        list: Vec<(BlockId, AskForBlocksInfo)>,
    ) -> Result<(), ProtocolError> {
        let node_info = match self.active_nodes.get_mut(&from_node_id) {
            Some(node_info) => node_info,
            _ => return Ok(()),
        };
        let mut all_blocks_info = vec![];
        for (hash, info_wanted) in &list {
            let operations_ids = match self.storage.read_blocks().get(hash) {
                Some(wrapped_block) => wrapped_block.content.operations.clone(),
                None => {
                    // let the node know we don't have the block.
                    all_blocks_info.push((*hash, ReplyForBlocksInfo::NotFound));
                    continue;
                }
            };
            let block_info = match info_wanted {
                AskForBlocksInfo::Info => ReplyForBlocksInfo::Info(operations_ids),
                AskForBlocksInfo::Operations(op_ids) => {
                    // Mark the node as having the block.
                    node_info.insert_known_blocks(
                        &[*hash],
                        true,
                        Instant::now(),
                        self.config.max_node_known_blocks_size,
                    );

                    // Send only the missing operations.
                    let needed_ops = operations_ids
                        .into_iter()
                        .filter(|id| op_ids.contains(id))
                        .collect();
                    ReplyForBlocksInfo::Operations(needed_ops)
                }
            };
            all_blocks_info.push((*hash, block_info));
        }
        self.network_command_sender
            .send_block_info(from_node_id, all_blocks_info)
            .await
            .map_err(|_| {
                ProtocolError::ChannelError("send block info network command send failed".into())
            })
    }

    /// On block information received, manage when we get a list of operations.
    /// Ask for the missing operations that are not in the `checked_operations` cache variable.
    ///
    /// # Ban
    /// Start compute the operations serialized total size with the operation we know.
    /// Ban the node if the operations contained in the block overflow the max size. We don't
    /// forward the block to the graph in that case.
    ///
    /// # Parameters:
    /// - `from_node_id`: Node which sent us the information.
    /// - `BlockId`: ID of the related operations we received.
    /// - `operation_ids`: IDs of the operations contained by the block.
    ///
    /// # Result
    /// return an error if stopping asking block failed. The error should be forwarded at the
    /// root. todo: check if if make panic.
    async fn on_block_operation_list_received(
        &mut self,
        from_node_id: NodeId,
        block_id: BlockId,
        operation_ids: Vec<OperationId>,
    ) -> Result<(), ProtocolError> {
        // add to known ops
        if let Some(node_info) = self.active_nodes.get_mut(&from_node_id) {
            node_info.insert_known_ops(
                operation_ids.iter().cloned().collect(),
                self.config.max_node_known_ops_size,
            );
        }

        match self.block_wishlist.get(&block_id) {
            Some(AskForBlocksInfo::Info) => {}
            _ => return Ok(()),
        }
        let mut info = match self.checked_headers.get_mut(&block_id) {
            Some(info) => info,
            _ => {
                warn!("Missing block header for {}", block_id);
                return Ok(());
            }
        };
        let mut total_hash: Vec<u8> = vec![];
        operation_ids.iter().for_each(|op_id| {
            let op_hash = op_id.hash().into_bytes();
            total_hash.extend(op_hash);
        });

        // Check operation_list against expected operations hash from header.
        if info.header.content.operation_merkle_root == Hash::compute_from(&total_hash) {
            // Add the ops of info.
            info.operations = Some(operation_ids.clone());

            let missing_operations = {
                let ops = self.storage.read_operations();
                operation_ids
                    .into_iter()
                    .filter(|op| {
                        // Filter missing operations returning true if not found and compute
                        // the sum of operation bytes size
                        if let Some(operation_id) = self.checked_operations.get(&op.into_prefix()) {
                            let op = ops
                                .get(operation_id)
                                .expect("unexpected checked operation not in storage");
                            info.operations_size =
                                info.operations_size.saturating_add(op.serialized_size());
                            return false;
                        }
                        true
                    })
                    .collect()
            };

            if info.operations_size > self.config.max_serialized_operations_size_per_block {
                let _ = self.ban_node(&from_node_id).await;
                return Ok(());
            }

            // Update ask block
            let mut set = Set::<BlockId>::with_capacity_and_hasher(1, BuildMap::default());
            set.insert(block_id);
            self.stop_asking_blocks(set)?;

            // Re-add to wishlist with new state.
            self.block_wishlist
                .insert(block_id, AskForBlocksInfo::Operations(missing_operations));
        } else {
            let _ = self.ban_node(&from_node_id).await;
        }
        Ok(())
    }

    /// Checks full block operations that we asked. (Because their was missing in the
    /// `checked_operations` cache variable, refer to `on_block_operation_list_received`)
    ///
    /// # Ban
    /// Ban the node if it doesn't fill the requirement. Forward to the graph with a
    /// [ProtocolEvent::ReceivedBlock] if the operations are under a max size.
    ///
    /// - thread incorect for an operation
    /// - wanted operations doesn't match
    /// - duplicated operation
    /// - full operations serialized size overflow
    ///
    /// We received these operation because we asked for the missing operation
    async fn on_block_full_operations_received(
        &mut self,
        from_node_id: NodeId,
        block_id: BlockId,
        operations: Vec<WrappedOperation>,
    ) -> Result<(), ProtocolError> {
        self.note_operations_from_node(operations.clone(), &from_node_id)?;

        let wanted_operation_ids: Set<OperationId> = match self.block_wishlist.get(&block_id) {
            Some(AskForBlocksInfo::Operations(ids)) => ids.clone().into_iter().collect(),
            _ => return Ok(()),
        };
        let info = match self.checked_headers.get(&block_id) {
            Some(info) => info,
            _ => {
                warn!("Missing block info for {}", block_id);
                return Ok(());
            }
        };

        let mut received_ids: Set<OperationId> = Default::default();
        let mut full_op_size = info.operations_size;

        // Ban the node if:
        // - thread incorect for an operation
        // - wanted operations doesn't match
        // - duplicated operation
        // - full operations serialized size overflow
        for op in operations.iter() {
            full_op_size = full_op_size.saturating_add(op.serialized_size());
            if op.thread != info.header.content.slot.thread
                || !received_ids.insert(op.id)
                || full_op_size > self.config.max_serialized_operations_size_per_block
            {
                let _ = self.ban_node(&from_node_id).await;
                return Ok(());
            }
        }
        if wanted_operation_ids != received_ids {
            let _ = self.ban_node(&from_node_id).await;
            return Ok(());
        }

        // Re-constitute block.
        let block = Block {
            header: info.header.clone(),
            operations: info.operations.clone().unwrap(),
        };

        let mut content_serialized = Vec::new();
        BlockSerializer::new() // todo : usage of constants would avoid a lot of instanciations
            .serialize(&block, &mut content_serialized)
            .unwrap();

        // wrap block
        let wrapped_block: WrappedBlock = Wrapped {
            signature: info.header.signature,
            creator_public_key: info.header.creator_public_key,
            creator_address: info.header.creator_address,
            thread: info.header.thread,
            id: block_id,
            content: block,
            serialized_data: content_serialized,
        };

        // create block storage (without parents)
        let mut block_storage = self.storage.clone_without_refs();

        // add block to local storage and claim ref
        block_storage.store_block(wrapped_block);

        // add operations to local storage and claim ref
        block_storage.store_operations(operations);

        // add endorsements to local storage and claim ref
        // TODO change this if we make endorsements separate from block header
        block_storage.store_endorsements(info.header.content.endorsements.clone());

        // Send to graph
        self.send_protocol_event(ProtocolEvent::ReceivedBlock {
            slot: info.header.content.slot,
            block_id,
            storage: block_storage,
        })
        .await;

        // Update ask block
        let mut set = Set::<BlockId>::with_capacity_and_hasher(1, BuildMap::default());
        set.insert(block_id);
        self.stop_asking_blocks(set)
    }

    async fn on_block_info_received(
        &mut self,
        from_node_id: NodeId,
        block_id: BlockId,
        info: BlockInfoReply,
    ) -> Result<(), ProtocolError> {
        match info {
            BlockInfoReply::Info(operation_list) => {
                // Ask for missing operations ids and print a warning if there is no header for
                // that block.
                // Ban the node if the operation ids hash doesn't match with the hash contained in
                // the block_header.
                self.on_block_operation_list_received(from_node_id, block_id, operation_list)
                    .await
            }
            BlockInfoReply::Operations(operations) => {
                // Send operations to pool,
                // before performing the below checks,
                // and wait for them to have been procesed(i.e. added to storage).
                self.on_block_full_operations_received(from_node_id, block_id, operations)
                    .await
            }
            BlockInfoReply::NotFound => {
                if let Some(info) = self.active_nodes.get_mut(&from_node_id) {
                    info.insert_known_blocks(
                        &[block_id],
                        false,
                        Instant::now(),
                        self.config.max_node_known_blocks_size,
                    );
                }
                Ok(())
            }
        }
    }
}
