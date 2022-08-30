//! Worker implementation for the network events
//!
//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::node_info::NodeInfo;
use crate::protocol_worker::ProtocolWorker;
use massa_hash::Hash;
use massa_logging::massa_trace;
use massa_models::{
    block::{Block, WrappedBlock},
    block::{BlockId, BlockSerializer},
    node::NodeId,
    operation::{OperationId, WrappedOperation},
    prehash::{CapacityAllocator, PreHashSet},
    wrapped::{Id, Wrapped},
};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NetworkEvent};
use massa_protocol_exports::{ProtocolError, ProtocolEvent};
use massa_serialization::Serializer;
use massa_storage::Storage;
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
                    all_blocks_info.push((*hash, BlockInfoReply::NotFound));
                    continue;
                }
            };
            let block_info = match info_wanted {
                AskForBlocksInfo::Info => BlockInfoReply::Info(operations_ids),
                AskForBlocksInfo::Operations(op_ids) => {
                    // Mark the node as having the block.
                    node_info.insert_known_blocks(
                        &[*hash],
                        true,
                        Instant::now(),
                        self.config.max_node_known_blocks_size,
                    );

                    // Send only the missing operations that is in storage.
                    let needed_ops = {
                        let operations = self.storage.read_operations();
                        operations_ids
                            .into_iter()
                            .filter(|id| op_ids.contains(id))
                            .filter_map(|id| operations.get(&id))
                            .cloned()
                            .collect()
                    };
                    BlockInfoReply::Operations(needed_ops)
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

    /// Return the sum of all operation's serialized sizes in the Set<Id>
    fn get_total_operations_size(
        storage: &Storage,
        operation_ids: &PreHashSet<OperationId>,
    ) -> usize {
        let op_reader = storage.read_operations();
        let mut total: usize = 0;
        operation_ids.iter().for_each(|id| {
            if let Some(op) = op_reader.get(id) {
                total = total.saturating_add(op.serialized_size());
            }
        });
        total
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
        // All operation ids sent into a set
        let mut operation_ids_set: PreHashSet<OperationId> =
            operation_ids.iter().cloned().collect();

        // add to known ops
        if let Some(node_info) = self.active_nodes.get_mut(&from_node_id) {
            node_info.insert_known_ops(
                operation_ids_set.clone(),
                self.config.max_node_known_ops_size,
            );
        }

        if self.block_wishlist.get(&block_id).is_none() {
            return Ok(());
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
            let op_hash = op_id.get_hash().into_bytes();
            total_hash.extend(op_hash);
        });

        // Check operation_list against expected operations hash from header.
        info!("AURELIEN: Merkle root1: {:#?}", info.header.content.operation_merkle_root);
        info!("AURELIEN: Merkle root2: {:#?}", Hash::compute_from(&total_hash));
        if info.header.content.operation_merkle_root == Hash::compute_from(&total_hash) {
            // Add the ops of info.
            info.operations = Some(operation_ids.clone());
            let mut block_storage = self.storage.clone_without_refs();
            let known_operations = block_storage.claim_operation_refs(&operation_ids_set);
            // remember the claimed operation to prune them later
            self.checked_operations.extend(&known_operations);
            // Compute the missing operations using `operation_ids_set`
            let mut missing_operation = std::mem::take(&mut operation_ids_set);
            missing_operation.retain(|id| !known_operations.contains(id));

            info.operations_size =
                Self::get_total_operations_size(&self.storage, &known_operations);

            if info.operations_size > self.config.max_serialized_operations_size_per_block {
                info!("bad operations size");
                let _ = self.ban_node(&from_node_id).await;
                return Ok(());
            }

            // Update ask block
            let mut set = PreHashSet::<BlockId>::with_capacity(1);
            set.insert(block_id);
            self.stop_asking_blocks(set)?;

            // Re-add to wishlist with new state.
            self.block_wishlist.insert(
                block_id,
                (
                    AskForBlocksInfo::Operations(missing_operation.into_iter().collect()),
                    Some(block_storage),
                ),
            );

            // If the block is empty, go straight to processing the full block info.
            if operation_ids.is_empty() {
                return self
                    .on_block_full_operations_received(from_node_id, block_id, Default::default())
                    .await;
            }
        } else {
            info!("bad merkle root");
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
        if self
            .note_operations_from_node(operations.clone(), &from_node_id)
            .is_err()
        {
            info!("bad operations full received");
            let _ = self.ban_node(&from_node_id).await;
            return Ok(());
        }

        let wanted_operation_ids = match self.block_wishlist.get(&block_id) {
            Some((AskForBlocksInfo::Operations(ids), Some(_))) => {
                ids.clone().into_iter().collect::<PreHashSet<OperationId>>()
            }
            _ => return Ok(()),
        };

        let info = match self.checked_headers.get(&block_id) {
            Some(info) => info,
            _ => {
                warn!("Missing block info for {}", block_id);
                return Ok(());
            }
        };

        let mut received_ids: PreHashSet<OperationId> = Default::default();
        let mut full_op_size = info.operations_size;

        // Ban the node if:
        // - thread incorrect for an operation
        // - wanted operations doesn't match
        // - duplicated operation
        // - full operations serialized size overflow
        for op in operations.iter() {
            full_op_size = full_op_size.saturating_add(op.serialized_size());
            let op_thread = op.creator_address.get_thread(self.config.thread_count);
            if op_thread != info.header.content.slot.thread
                || !received_ids.insert(op.id)
                || full_op_size > self.config.max_serialized_operations_size_per_block
            {
                info!("bad operations full 2");
                let _ = self.ban_node(&from_node_id).await;
                return Ok(());
            }
        }
        if wanted_operation_ids != received_ids {
            info!("not all id error");
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
            id: block_id,
            content: block,
            serialized_data: content_serialized,
        };

        // create block storage (without parents)
        let mut block_storage = self.block_wishlist.remove(&block_id).unwrap().1.unwrap();
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
        let mut set = PreHashSet::<BlockId>::with_capacity(1);
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
