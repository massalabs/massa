//! Worker implementation for the network events
//!
//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::hash_map::Entry;

use crate::node_info::NodeInfo;
use crate::protocol_worker::ProtocolWorker;
use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_logging::massa_trace;
use massa_models::{
    block::{Block, BlockSerializer},
    block_header::SecuredHeader,
    block_id::BlockId,
    node::NodeId,
    operation::{OperationId, SecureShareOperation},
    prehash::{CapacityAllocator, PreHashSet},
    secure_share::{Id, SecureShare},
};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply, NetworkEvent};
use massa_protocol_exports::ProtocolError;
use massa_serialization::Serializer;
use massa_storage::Storage;
use std::pin::Pin;
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
        block_ask_timer: &mut Pin<&mut Sleep>,
        op_timer: &mut Pin<&mut Sleep>,
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
                    if self.active_nodes.is_empty() {
                        // if no more active nodes, print
                        info!("Not connected to any peers.");
                    }
                    self.update_ask_block(block_ask_timer).await?;
                }
            }
            NetworkEvent::ReceivedBlockInfo {
                node: from_node_id,
                info,
            } => {
                massa_trace!(BLOCKS_INFO, { "node": from_node_id, "info": info });
                for (block_id, block_info) in info.into_iter() {
                    self.on_block_info_received(from_node_id, block_id, block_info, op_timer)
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
                if let Some((block_id, is_new)) =
                    self.note_header_from_node(&header, &source_node_id).await?
                {
                    if is_new {
                        self.consensus_controller
                            .register_block_header(block_id, header);
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
                self.on_operations_received(node, operations, op_timer)
                    .await;
            }
            NetworkEvent::ReceivedEndorsements { node, endorsements } => {
                massa_trace!(ENDORSEMENTS, { "node": node, "endorsements": endorsements});
                if let Err(err) = self
                    .note_endorsements_from_node(endorsements, &node, true)
                    .await
                {
                    warn!(
                        "node {} sent us critically incorrect endorsements, \
                        which may be an attack attempt by the remote node or a \
                        loss of sync between us and the remote node. Err = {}",
                        node, err
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
    /// React on another node asking for blocks information. We can forward the operation ids if
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
            let (header, operations_ids) = match self.storage.read_blocks().get(hash) {
                Some(signed_block) => (
                    signed_block.content.header.clone(),
                    signed_block.content.operations.clone(),
                ),
                None => {
                    // let the node know we don't have the block.
                    all_blocks_info.push((*hash, BlockInfoReply::NotFound));
                    continue;
                }
            };
            let block_info = match info_wanted {
                AskForBlocksInfo::Header => BlockInfoReply::Header(header),
                AskForBlocksInfo::Info => BlockInfoReply::Info(operations_ids),
                AskForBlocksInfo::Operations(op_ids) => {
                    // Mark the node as having the block.
                    node_info.insert_known_blocks(
                        &[*hash],
                        true,
                        Instant::now(),
                        self.config.max_node_known_blocks_size,
                    );

                    // Send only the missing operations that are in storage.
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

    /// On block header received from a node.
    /// If the header is new, we propagate it to the consensus.
    /// We pass the state of `block_wishlist` to ask for information about the block.
    async fn on_block_header_received(
        &mut self,
        from_node_id: NodeId,
        block_id: BlockId,
        header: SecuredHeader,
    ) -> Result<(), ProtocolError> {
        if let Some(info) = self.block_wishlist.get(&block_id) {
            if info.header.is_some() {
                warn!(
                    "Node {} sent us header for block id {} but we already received it.",
                    from_node_id, block_id
                );
                if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                    node.asked_blocks.remove(&block_id);
                    node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
                }

                return Ok(());
            }
        }
        if let Err(err) = self.note_header_from_node(&header, &from_node_id).await {
            warn!(
                "node {} sent us critically incorrect header through protocol, \
                which may be an attack attempt by the remote node \
                or a loss of sync between us and the remote node. Err = {}",
                from_node_id, err
            );
            let _ = self.ban_node(&from_node_id).await;
            return Ok(());
        };
        if let Some(info) = self.block_wishlist.get_mut(&block_id) {
            info.header = Some(header);
        }

        // Update ask block
        let mut set = PreHashSet::<BlockId>::with_capacity(1);
        set.insert(block_id);
        self.remove_asked_blocks_of_node(&set)?;
        Ok(())
    }

    /// On block information received, manage when we get a list of operations.
    /// Ask for the missing operations that are not in the `checked_operations` cache variable.
    ///
    /// # Ban
    /// Start compute the operations serialized total size with the operation we know.
    /// Ban the node if the operations contained in the block overflow the max size. We don't
    /// forward the block to the consensus in that case.
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
        op_timer: &mut Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        // All operation ids sent into a set
        let operation_ids_set: PreHashSet<OperationId> = operation_ids.iter().cloned().collect();

        // add to known ops
        if let Some(node_info) = self.active_nodes.get_mut(&from_node_id) {
            node_info.insert_known_ops(operation_ids.iter().map(|id| id.prefix()));
        }

        let info = if let Some(info) = self.block_wishlist.get_mut(&block_id) {
            info
        } else {
            warn!(
                "Node {} sent us an operation list but we don't have block id {} in our wishlist.",
                from_node_id, block_id
            );
            if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                node.asked_blocks.remove(&block_id);
                node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
            }
            return Ok(());
        };

        let header = if let Some(header) = &info.header {
            header
        } else {
            warn!("Node {} sent us an operation list but we don't have receive the header of block id {} yet.", from_node_id, block_id);
            if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                node.asked_blocks.remove(&block_id);
                node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
            }
            return Ok(());
        };

        if info.operation_ids.is_some() {
            warn!(
                "Node {} sent us an operation list for block id {} but we already received it.",
                from_node_id, block_id
            );
            if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                node.asked_blocks.remove(&block_id);
                node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
            }
            return Ok(());
        }

        let mut total_hash: Vec<u8> =
            Vec::with_capacity(operation_ids.len().saturating_mul(HASH_SIZE_BYTES));
        operation_ids.iter().for_each(|op_id| {
            let op_hash = op_id.get_hash().into_bytes();
            total_hash.extend(op_hash);
        });

        // Check operation_list against expected operations hash from header.
        if header.content.operation_merkle_root == Hash::compute_from(&total_hash) {

            if operation_ids.len() > self.config.max_operations_per_block as usize {
                warn!("Node id {} sent us a operation list for block id {} but the operations we already have in our records exceed max operations per block constant.", from_node_id, block_id);
                let _ = self.ban_node(&from_node_id).await;
                return Ok(());
            }

            // Add the ops of info.
            info.operation_ids = Some(operation_ids.clone());
            let known_operations = info.storage.claim_operation_refs(&operation_ids_set);

            // get the total size of known ops
            info.operations_size =
                Self::get_total_operations_size(&self.storage, &known_operations);

            // mark ops as checked
            self.checked_operations
                .extend(known_operations.iter().copied());

            if info.operations_size > self.config.max_serialized_operations_size_per_block {
                warn!("Node id {} sent us a operation list for block id {} but the operations we already have in our records exceed max size.", from_node_id, block_id);
                let _ = self.ban_node(&from_node_id).await;
                return Ok(());
            }


            // Update ask block
            let mut set = PreHashSet::<BlockId>::with_capacity(1);
            set.insert(block_id);
            self.remove_asked_blocks_of_node(&set)?;

            // If the block is empty, go straight to processing the full block info.
            if operation_ids.is_empty() {
                return self
                    .on_block_full_operations_received(
                        from_node_id,
                        block_id,
                        Default::default(),
                        op_timer,
                    )
                    .await;
            }
        } else {
            warn!("Node id {} sent us a operation list for block id {} but the hash in header doesn't match.", from_node_id, block_id);
            let _ = self.ban_node(&from_node_id).await;
        }
        Ok(())
    }

    /// Checks full block operations that we asked. (Because their was missing in the
    /// `checked_operations` cache variable, refer to `on_block_operation_list_received`)
    ///
    /// # Ban
    /// Ban the node if it doesn't fill the requirement. Forward to the graph with a
    /// `ProtocolEvent::ReceivedBlock` if the operations are under a max size.
    ///
    /// - thread incorrect for an operation
    /// - wanted operations doesn't match
    /// - duplicated operation
    /// - full operations serialized size overflow
    ///
    /// We received these operation because we asked for the missing operation
    async fn on_block_full_operations_received(
        &mut self,
        from_node_id: NodeId,
        block_id: BlockId,
        mut operations: Vec<SecureShareOperation>,
        op_timer: &mut Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        if let Err(err) = self
            .note_operations_from_node(operations.clone(), &from_node_id, op_timer)
            .await
        {
            warn!(
                "Node id {} sent us operations for block id {} but they failed at verifications. Err = {}",
                from_node_id, block_id, err
            );
            let _ = self.ban_node(&from_node_id).await;
            return Ok(());
        }

        match self.block_wishlist.entry(block_id) {
            Entry::Occupied(mut entry) => {
                let info = entry.get_mut();
                let header = if let Some(header) = &info.header {
                    header.clone()
                } else {
                    warn!("Node {} sent us full operations but we don't have receive the header of block id {} yet.", from_node_id, block_id);
                    if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                        node.asked_blocks.remove(&block_id);
                        node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
                    }
                    return Ok(());
                };
                let block_operation_ids = if let Some(operations) = &info.operation_ids {
                    operations
                } else {
                    warn!("Node {} sent us full operations but we don't have received the operation list of block id {} yet.", from_node_id, block_id);
                    if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                        node.asked_blocks.remove(&block_id);
                        node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
                    }
                    return Ok(());
                };
                operations.retain(|op| block_operation_ids.contains(&op.id));
                // add operations to local storage and claim ref
                info.storage.store_operations(operations);
                let block_ids_set = block_operation_ids.clone().into_iter().collect();
                let known_operations = info.storage.claim_operation_refs(&block_ids_set);

                // Ban the node if:
                // - mismatch with asked operations (asked operations are the one that are not in storage) + operations already in storage and block operations
                // - full operations serialized size overflow
                let full_op_size: usize = {
                    let stored_operations = info.storage.read_operations();
                    known_operations
                        .iter()
                        .map(|id| stored_operations.get(id).unwrap().serialized_size())
                        .sum()
                };
                if full_op_size > self.config.max_serialized_operations_size_per_block {
                    warn!("Node id {} sent us full operations for block id {} but they exceed max size.", from_node_id, block_id);
                    let _ = self.ban_node(&from_node_id).await;
                    self.block_wishlist.remove(&block_id);
                    self.consensus_controller
                        .mark_invalid_block(block_id, header);
                } else {
                    if known_operations != block_ids_set {
                        warn!(
                            "Node id {} didn't sent us all the full operations for block id {}.",
                            from_node_id, block_id
                        );
                        if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                            node.asked_blocks.remove(&block_id);
                            node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
                        }
                        return Ok(());
                    }

                    // Re-constitute block.
                    let block = Block {
                        header: header.clone(),
                        operations: block_operation_ids.clone(),
                    };

                    let mut content_serialized = Vec::new();
                    BlockSerializer::new() // todo : keep the serializer in the struct to avoid recreating it
                        .serialize(&block, &mut content_serialized)
                        .unwrap();

                    // wrap block
                    let signed_block = SecureShare {
                        signature: header.signature,
                        content_creator_pub_key: header.content_creator_pub_key,
                        content_creator_address: header.content_creator_address,
                        id: block_id,
                        content: block,
                        serialized_data: content_serialized,
                    };

                    // create block storage (without parents)
                    let mut block_storage = entry.remove().storage;
                    // add endorsements to local storage and claim ref
                    // TODO change this if we make endorsements separate from block header
                    block_storage.store_endorsements(
                        signed_block.content.header.content.endorsements.clone(),
                    );
                    let slot = signed_block.content.header.content.slot;
                    // add block to local storage and claim ref
                    block_storage.store_block(signed_block);

                    // Send to consensus
                    self.consensus_controller
                        .register_block(block_id, slot, block_storage, false);
                }
            }
            Entry::Vacant(_) => {
                warn!("Node {} sent us full operations but we don't have the block id {} in our wishlist.", from_node_id, block_id);
                if let Some(node) = self.active_nodes.get_mut(&from_node_id) && node.asked_blocks.contains_key(&block_id) {
                    node.asked_blocks.remove(&block_id);
                    node.insert_known_blocks(&[block_id], false, Instant::now(), self.config.max_node_known_blocks_size);
                }
                return Ok(());
            }
        };

        // Update ask block
        let remove_hashes = vec![block_id].into_iter().collect();
        self.remove_asked_blocks_of_node(&remove_hashes)
    }

    async fn on_block_info_received(
        &mut self,
        from_node_id: NodeId,
        block_id: BlockId,
        info: BlockInfoReply,
        op_timer: &mut Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        match info {
            BlockInfoReply::Header(header) => {
                // Verify and Send it consensus
                self.on_block_header_received(from_node_id, block_id, header)
                    .await
            }
            BlockInfoReply::Info(operation_list) => {
                // Ask for missing operations ids and print a warning if there is no header for
                // that block.
                // Ban the node if the operation ids hash doesn't match with the hash contained in
                // the block_header.
                self.on_block_operation_list_received(
                    from_node_id,
                    block_id,
                    operation_list,
                    op_timer,
                )
                .await
            }
            BlockInfoReply::Operations(operations) => {
                // Send operations to pool,
                // before performing the below checks,
                // and wait for them to have been procesed(i.e. added to storage).
                self.on_block_full_operations_received(from_node_id, block_id, operations, op_timer)
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
