use std::{
    collections::{HashMap, HashSet},
    thread::JoinHandle,
    time::Instant,
};

use crate::{
    handlers::{
        endorsement_handler::{
            cache::SharedEndorsementCache,
            commands_propagation::EndorsementHandlerPropagationCommand,
            note_endorsements_from_peer,
        },
        operation_handler::{
            cache::SharedOperationCache, commands_propagation::OperationHandlerPropagationCommand,
        },
        peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
    },
    messages::{Message, MessagesSerializer},
    wrap_network::ActiveConnectionsTrait,
};
use crossbeam::{
    channel::{at, tick},
    select,
};
use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_consensus_exports::ConsensusController;
use massa_logging::massa_trace;
use massa_metrics::MassaMetrics;
use massa_models::{
    block::{Block, BlockSerializer},
    block_header::SecuredHeader,
    block_id::BlockId,
    endorsement::EndorsementId,
    operation::{OperationId, OperationIdSerializer, SecureShareOperation},
    prehash::{PreHashMap, PreHashSet},
    secure_share::SecureShare,
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_storage::Storage;
use massa_time::TimeError;
use massa_versioning::versioning::MipStore;
use rand::thread_rng;
use rand::{seq::SliceRandom, Rng};
use tracing::{debug, info, warn};

use super::{
    super::operation_handler::note_operations_from_peer,
    cache::SharedBlockCache,
    commands_propagation::BlockHandlerPropagationCommand,
    commands_retrieval::BlockHandlerRetrievalCommand,
    messages::{
        AskForBlockInfo, BlockInfoReply, BlockMessage, BlockMessageDeserializer,
        BlockMessageDeserializerArgs,
    },
    BlockMessageSerializer,
};

/// Info about a block we've seen
#[derive(Debug, Clone)]
pub(crate) struct BlockInfo {
    /// The header of the block.
    pub(crate) header: Option<SecuredHeader>,
    /// Operations ids. None if not received yet
    pub(crate) operation_ids: Option<Vec<OperationId>>,
    /// Operations and endorsements contained in the block,
    /// if we've received them already, and none otherwise.
    pub(crate) storage: Storage,
}

impl BlockInfo {
    fn new(header: Option<SecuredHeader>, storage: Storage) -> Self {
        BlockInfo {
            header,
            operation_ids: None,
            storage,
        }
    }
}

pub struct RetrievalThread {
    active_connections: Box<dyn ActiveConnectionsTrait>,
    selector_controller: Box<dyn SelectorController>,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    receiver_network: MassaReceiver<PeerMessageTuple>,
    _announcement_sender: MassaSender<BlockHandlerPropagationCommand>,
    receiver: MassaReceiver<BlockHandlerRetrievalCommand>,
    block_message_serializer: MessagesSerializer,
    block_wishlist: PreHashMap<BlockId, BlockInfo>,
    asked_blocks: HashMap<PeerId, PreHashMap<BlockId, Instant>>,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    sender_propagation_ops: MassaSender<OperationHandlerPropagationCommand>,
    sender_propagation_endorsements: MassaSender<EndorsementHandlerPropagationCommand>,
    endorsement_cache: SharedEndorsementCache,
    operation_cache: SharedOperationCache,
    next_timer_ask_block: Instant,
    cache: SharedBlockCache,
    config: ProtocolConfig,
    storage: Storage,
    mip_store: MipStore,
    massa_metrics: MassaMetrics,
}

impl RetrievalThread {
    fn run(&mut self) {
        let block_message_deserializer =
            BlockMessageDeserializer::new(BlockMessageDeserializerArgs {
                thread_count: self.config.thread_count,
                endorsement_count: self.config.endorsement_count,
                max_operations_per_block: self.config.max_operations_per_block,
                max_datastore_value_length: self.config.max_size_value_datastore,
                max_function_name_length: self.config.max_size_function_name,
                max_parameters_size: self.config.max_size_call_sc_parameter,
                max_op_datastore_entry_count: self.config.max_op_datastore_entry_count,
                max_op_datastore_key_length: self.config.max_op_datastore_key_length,
                max_op_datastore_value_length: self.config.max_op_datastore_value_length,
                max_denunciations_in_block_header: self.config.max_denunciations_in_block_header,
                last_start_period: Some(self.config.last_start_period),
            });

        let tick_update_metrics = tick(self.massa_metrics.tick_delay);
        loop {
            select! {
                recv(self.receiver_network) -> msg => {
                    self.receiver_network.update_metrics();
                    match msg {
                        Ok((peer_id, message)) => {
                            let (rest, message) = match block_message_deserializer
                                .deserialize::<DeserializeError>(&message) {
                                Ok((rest, message)) => (rest, message),
                                Err(err) => {
                                    warn!("Error in deserializing block message: {:?}", err);
                                    continue;
                                }
                            };
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                BlockMessage::DataRequest{block_id, block_info} => {
                                    self.on_ask_for_block_info_received(peer_id.clone(), block_id, block_info);
                                }
                                BlockMessage::DataResponse{block_id, block_info} => {
                                   self.on_block_info_received(peer_id.clone(), block_id, block_info);
                                   self.update_block_retrieval();
                                }
                                BlockMessage::Header(header) => {
                                    self.on_block_header_received(peer_id.clone(), header);
                                    self.update_block_retrieval();
                                }
                            }
                        },
                        Err(_) => {
                            info!("Stop block retrieval thread");
                            return;
                        }
                    }
                },
                recv(self.receiver) -> msg => {
                    self.receiver.update_metrics();
                    match msg {
                        Ok(command) => {
                            match command {
                                BlockHandlerRetrievalCommand::WishlistDelta { new, remove } => {
                                    massa_trace!("protocol.protocol_worker.process_command.wishlist_delta.begin", { "new": new, "remove": remove });
                                    for (block_id, header) in new.into_iter() {
                                        self.block_wishlist.insert(
                                            block_id,
                                            BlockInfo::new(header, self.storage.clone_without_refs()),
                                        );
                                    }
                                    // Cleanup the knowledge that we asked this list of blocks to nodes.
                                    self.remove_asked_blocks(&remove);

                                    // Remove from the wishlist.
                                    for block_id in remove.iter() {
                                        self.block_wishlist.remove(block_id);
                                    }

                                    // update block asking process
                                    self.update_block_retrieval();
                                },
                                BlockHandlerRetrievalCommand::Stop => {
                                    info!("Stop block retrieval thread from command receiver (Stop)");
                                    return;
                                }
                            }
                        },
                        Err(_) => {
                            info!("Stop block retrieval thread from command receiver");
                            return;
                        }
                    }
                },
                recv(tick_update_metrics) -> _ => {
                    // update metrics
                    {
                        let block_read = self.cache.read();

                        self.massa_metrics.set_block_cache_metrics(
                            block_read.checked_headers.len(),
                            block_read.blocks_known_by_peer.len(),
                        );
                    }

                    {
                        let ope_read = self.operation_cache.read();
                        self.massa_metrics.set_operations_cache_metrics(
                            ope_read.checked_operations.len(),
                            ope_read.checked_operations_prefix.len(),
                            ope_read.ops_known_by_peer.len(),
                        );
                    }
                }
                recv(at(self.next_timer_ask_block)) -> _ => {
                    self.update_block_retrieval();
                }
            }
        }
    }

    /// A remote node asked the local node for block data
    ///
    /// We send the block's operation ids if the foreign node asked for `AskForBlockInfo::Info`
    /// or a subset of the full operations of the block if it asked for `AskForBlockInfo::Operations`.
    fn on_ask_for_block_info_received(
        &mut self,
        from_peer_id: PeerId,
        block_id: BlockId,
        info_requested: AskForBlockInfo,
    ) {
        // updates on the remote peer's knowledge on blocks, operations and endorsements
        // only applied if the response is successfully sent to the peer
        let mut block_knowledge_updates = PreHashSet::default();
        let mut operation_knowledge_updates = PreHashSet::default();
        let mut endorsement_knowledge_updates = PreHashSet::default();

        // retrieve block data from storage
        let stored_header_op_ids = self.storage.read_blocks().get(&block_id).map(|block| {
            (
                block.content.header.clone(),
                block.content.operations.clone(),
            )
        });

        let block_info_response = match (stored_header_op_ids, info_requested) {
            (None, _) => BlockInfoReply::NotFound,

            (Some((header, _)), AskForBlockInfo::Header) => {
                // the peer asked for a block header

                // once sent, the peer will know about that block,
                // no need to announce this header to that peer anymore
                block_knowledge_updates.insert(block_id);

                // once sent, the peer will know about the endorsements in that block,
                // no need to announce those endorsements to that peer anymore
                endorsement_knowledge_updates.extend(
                    header
                        .content
                        .endorsements
                        .iter()
                        .map(|e| e.id)
                        .collect::<PreHashSet<EndorsementId>>(),
                );

                BlockInfoReply::Header(header)
            }
            (Some((_, block_op_ids)), AskForBlockInfo::OperationIds) => {
                // the peer asked for the operation IDs of the block

                // once sent, the peer will know about those operations,
                // no need to announce their IDs to that peer anymore
                operation_knowledge_updates.extend(block_op_ids.iter().cloned());

                BlockInfoReply::OperationIds(block_op_ids)
            }
            (Some((_, block_op_ids)), AskForBlockInfo::Operations(mut asked_ops)) => {
                // the peer asked for a list of full operations from the block

                // retain only ops that belong to the block
                {
                    let block_op_ids_set: PreHashSet<OperationId> =
                        block_op_ids.iter().copied().collect();
                    asked_ops.retain(|id| block_op_ids_set.contains(id));
                }

                // Send the operations that are available in storage
                let returned_ops: Vec<_> = {
                    let op_storage_lock = self.storage.read_operations();
                    asked_ops
                        .into_iter()
                        .filter_map(|id| op_storage_lock.get(&id))
                        .cloned()
                        .collect()
                };

                // mark the peer as knowing about those operations,
                // no need to announce their IDs to them anymore
                operation_knowledge_updates.extend(
                    returned_ops
                        .iter()
                        .map(|op| op.id)
                        .collect::<PreHashSet<OperationId>>(),
                );

                BlockInfoReply::Operations(returned_ops)
            }
        };

        debug!("Send reply for block info to {}", from_peer_id);

        // send response to peer
        if let Err(err) = self.active_connections.send_to_peer(
            &from_peer_id,
            &self.block_message_serializer,
            BlockMessage::DataResponse {
                block_id,
                block_info: block_info_response,
            }
            .into(),
            true,
        ) {
            warn!(
                "Error while sending reply for blocks to {}: {:?}",
                from_peer_id, err
            );
            return;
        }

        // here we know that the response was successfully sent to the peer
        // so we can update our vision of the peer's knowledge on blocks, operations and endorsements
        if !block_knowledge_updates.is_empty() {
            self.cache.write().insert_peer_known_block(
                &from_peer_id,
                &block_knowledge_updates.into_iter().collect::<Vec<_>>(),
                true,
            );
        }
        if !operation_knowledge_updates.is_empty() {
            self.operation_cache.write().insert_peer_known_ops(
                &from_peer_id,
                &operation_knowledge_updates
                    .into_iter()
                    .map(|op_id| op_id.prefix())
                    .collect::<Vec<_>>(),
            );
        }
        if !endorsement_knowledge_updates.is_empty() {
            self.endorsement_cache
                .write()
                .insert_peer_known_endorsements(
                    &from_peer_id,
                    &endorsement_knowledge_updates
                        .into_iter()
                        .collect::<Vec<_>>(),
                );
        }
    }

    /// A peer sent us a response to one of our requests for block data
    fn on_block_info_received(
        &mut self,
        from_peer_id: PeerId,
        block_id: BlockId,
        block_info: BlockInfoReply,
    ) {
        match block_info {
            BlockInfoReply::Header(header) => {
                // Verify and send it consensus
                self.on_block_header_received(from_peer_id, header);
            }
            BlockInfoReply::OperationIds(operation_list) => {
                // Ask for missing operations ids and print a warning if there is no header for
                // that block.
                // Ban the node if the operation ids hash doesn't match with the hash contained in
                // the block_header.
                self.on_block_operation_list_received(from_peer_id, block_id, operation_list);
            }
            BlockInfoReply::Operations(operations) => {
                // Send operations to pool,
                // before performing the below checks,
                // and wait for them to have been procesed(i.e. added to storage).
                self.on_block_full_operations_received(from_peer_id, block_id, operations);
            }
            BlockInfoReply::NotFound => {
                // The peer doesn't know about the block. Mark it as such.
                self.cache
                    .write()
                    .insert_peer_known_block(&from_peer_id, &[block_id], false);
            }
        }
    }

    /// On block header received from a node.
    fn on_block_header_received(&mut self, from_peer_id: PeerId, header: SecuredHeader) {
        let block_id = header.id;

        // Check header and update knowledge info
        let is_new = match self.note_header_from_peer(&header, &from_peer_id) {
            Ok(is_new) => is_new,
            Err(err) => {
                warn!(
                    "peer {} sent us critically incorrect header: {}",
                    &from_peer_id, err
                );
                if let Err(err) = self.ban_peers(&[from_peer_id.clone()]) {
                    warn!("Error while banning peer {} err: {:?}", &from_peer_id, err);
                }
                return;
            }
        };

        if let Some(info) = self.block_wishlist.get_mut(&block_id) {
            // We are actively trying to get this block

            if info.header.is_none() {
                // we were looking for the missing header

                // save the header
                info.header = Some(header);

                // Clear the list of peers we asked that header for.
                // This is done so that update_block_retrieval can prioritize asking the rest of the block data
                // to that same peer that just gave us the header, and not exclude the peer
                // because we still believe we are actively asking it for stuff.
                self.remove_asked_blocks(&[block_id].into_iter().collect())
            }
        } else if is_new {
            // if not in wishlist, and if the header is new, we send it to consensus
            self.consensus_controller
                .register_block_header(block_id, header);
        }
    }

    /// Check if the incoming header network version is compatible with the current node
    fn check_network_version_compatibility(
        &self,
        header: &SecuredHeader,
    ) -> Result<(), ProtocolError> {
        let slot = header.content.slot;
        let timestamp = get_block_slot_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            slot,
        )?;

        let current_version = self.mip_store.get_network_version_active_at(timestamp);
        if header.content.current_version != current_version {
            // Received a block version different from current version given by mip store
            return Err(ProtocolError::IncompatibleNetworkVersion {
                local: current_version,
                received: header.content.current_version,
            });
        }

        if let Some(announced_version) = header.content.announced_version {
            if announced_version <= current_version {
                // Received an announced network version that is already known
                return Err(ProtocolError::OutdatedAnnouncedNetworkVersion {
                    local: current_version,
                    announced_received: announced_version,
                });
            }
        }

        Ok(())
    }

    /// Performs validity checks on a block header,
    /// and if valid update the node's view of its surrounding peers.
    ///
    /// Returns a boolean indicating whether the header is new.
    ///
    /// Does not ban the source node if the header is invalid.
    ///
    /// Checks performed on Header:
    /// - Not genesis
    /// - Compatible version
    /// - Can compute a `BlockId`
    /// - Valid signature
    /// - All endorsement are valid
    /// - Endorsements have unique indices
    /// - Endorsement slots match that of the block
    /// - Endorsed blocks match the same-thread parent of the header
    pub(crate) fn note_header_from_peer(
        &mut self,
        header: &SecuredHeader,
        from_peer_id: &PeerId,
    ) -> Result<bool, ProtocolError> {
        // refuse genesis blocks
        if header.content.slot.period == 0 || header.content.parents.is_empty() {
            return Err(ProtocolError::InvalidBlock("block is genesis".to_string()));
        }

        // Check that our node supports the block version
        self.check_network_version_compatibility(header)?;

        let block_id = header.id;

        // check if the header has not been seen before (is_new == true)
        let is_new;
        {
            let mut cache_write = self.cache.write();
            is_new = cache_write.checked_headers.get(&block_id).is_none();
            if !is_new {
                // the header was previously verified

                // mark the sender peer as knowing the block and its parents
                cache_write.insert_peer_known_block(
                    from_peer_id,
                    &[&[block_id], header.content.parents.as_slice()].concat(),
                    true,
                );
            }
        }

        // if the header was previously verified, update peer knowledge information and return Ok(false)
        if !is_new {
            // mark the sender peer as knowing the endorsements in the block
            {
                let endorsement_ids: Vec<_> =
                    header.content.endorsements.iter().map(|e| e.id).collect();
                self.endorsement_cache
                    .write()
                    .insert_peer_known_endorsements(from_peer_id, &endorsement_ids);
            }

            // mark the sender peer as knowing the operations of the block (if we know them)
            let opt_block_ops: Option<Vec<_>> =
                self.storage.read_blocks().get(&block_id).map(|b| {
                    b.content
                        .operations
                        .iter()
                        .map(|op_id| op_id.prefix())
                        .collect()
                });
            if let Some(block_ops) = opt_block_ops {
                self.operation_cache
                    .write()
                    .insert_peer_known_ops(from_peer_id, &block_ops);
            }

            // return that we already know that header
            return Ok(false);
        }

        // check endorsements
        if let Err(err) = note_endorsements_from_peer(
            header.content.endorsements.clone(),
            from_peer_id,
            &self.endorsement_cache,
            &self.selector_controller,
            &self.storage,
            &self.config,
            &self.sender_propagation_endorsements,
            &mut self.pool_controller,
        ) {
            return Err(ProtocolError::InvalidBlock(format!(
                "invalid endorsements: {}",
                err
            )));
        };

        // check header signature
        if let Err(err) = header.verify_signature() {
            return Err(ProtocolError::InvalidBlock(format!(
                "invalid header signature: {}",
                err
            )));
        };

        // check endorsement integrity within the context of the header
        let mut used_endorsement_indices: HashSet<u32> =
            HashSet::with_capacity(header.content.endorsements.len());
        for endorsement in header.content.endorsements.iter() {
            // check index reuse
            if !used_endorsement_indices.insert(endorsement.content.index) {
                return Err(ProtocolError::InvalidBlock(format!(
                    "duplicate endorsement index: {}",
                    endorsement.content.index
                )));
            }
            // check slot
            if endorsement.content.slot != header.content.slot {
                return Err(ProtocolError::InvalidBlock(format!(
                    "endorsement slot {} does not match header slot: {}",
                    endorsement.content.slot, header.content.slot
                )));
            }
            // check endorsed block
            if endorsement.content.endorsed_block
                != header.content.parents[header.content.slot.thread as usize]
            {
                return Err(ProtocolError::InvalidBlock(format!(
                    "endorsed block {} does not match header parent: {}",
                    endorsement.content.endorsed_block,
                    header.content.parents[header.content.slot.thread as usize]
                )));
            }
        }

        // mark the sender peer as knowing the endorsements in the block
        {
            let endorsement_ids: Vec<_> =
                header.content.endorsements.iter().map(|e| e.id).collect();
            self.endorsement_cache
                .write()
                .insert_peer_known_endorsements(from_peer_id, &endorsement_ids);
        }

        {
            let mut cache_lock = self.cache.write();

            // mark the sender peer as knowing the block and its parents
            cache_lock.insert_peer_known_block(
                from_peer_id,
                &[&[block_id], header.content.parents.as_slice()].concat(),
                true,
            );

            // mark us as knowing the header
            cache_lock.checked_headers.insert(block_id, header.clone());
        }

        Ok(true)
    }

    /// send a ban peer command to the peer handler
    fn ban_peers(&mut self, peer_ids: &[PeerId]) -> Result<(), ProtocolError> {
        self.peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(peer_ids.to_vec()))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }

    /// Remove the given blocks from the local wishlist
    pub(crate) fn remove_asked_blocks(&mut self, remove_hashes: &PreHashSet<BlockId>) {
        for asked_blocks in self.asked_blocks.values_mut() {
            for remove_h in remove_hashes {
                asked_blocks.remove(remove_h);
            }
        }
    }

    /// Mark a block as invalid
    fn mark_block_as_invalid(&mut self, block_id: &BlockId) {
        // stop retrieving the block
        if let Some(wishlist_info) = self.block_wishlist.remove(block_id) {
            if let Some(header) = wishlist_info.header {
                // notify consensus that the block is invalid
                self.consensus_controller
                    .mark_invalid_block(*block_id, header);
            }
        }

        // ban all peers that know about this block
        let mut peers_to_ban = Vec::new();
        {
            let cache_read = self.cache.read();
            for (peer_id, peer_known_blocks) in cache_read.blocks_known_by_peer.iter() {
                if peer_known_blocks.peek(block_id).is_some() {
                    peers_to_ban.push(peer_id.clone());
                }
            }
        }
        if !peers_to_ban.is_empty() {
            if let Err(err) = self.ban_peers(&peers_to_ban) {
                warn!(
                    "Error while banning peers {:?} err: {:?}",
                    peers_to_ban, err
                );
            }
        }

        // clear retrieval cache
        self.remove_asked_blocks(&[*block_id].into_iter().collect());
    }

    /// We received a list of operations for a block.
    ///
    /// # Parameters:
    /// - `from_peer_id`: Node which sent us the information.
    /// - `BlockId`: ID of the related operations we received.
    /// - `operation_ids`: IDs of the operations contained by the block, ordered and can contain duplicates.
    fn on_block_operation_list_received(
        &mut self,
        from_peer_id: PeerId,
        block_id: BlockId,
        operation_ids: Vec<OperationId>,
    ) {
        // Note that the length of the operation list was checked at deserialization to not overflow the max per block.

        // All operation ids sent into a set to deduplicate and search quickly for presence
        let operation_ids_set: PreHashSet<OperationId> = operation_ids.iter().cloned().collect();

        // mark the sender node as knowing those ops
        self.operation_cache.write().insert_peer_known_ops(
            &from_peer_id,
            &operation_ids_set
                .iter()
                .map(|op_id| op_id.prefix())
                .collect::<Vec<_>>(),
        );

        // check if we were looking to retrieve the list of ops for that block
        let wishlist_info = if let Some(info) = self.block_wishlist.get_mut(&block_id) && info.header.is_some() && info.operation_ids.is_none() {
            // we were actively looking for this data
            info
        } else {
            // we were not actively looking for that data, but mark the remote node as knowing the block
            debug!("peer {} sent us a list of operation IDs for block id {} but we were not looking for it", from_peer_id, block_id);
            self.cache.write().insert_peer_known_block(
                &from_peer_id,
                &[block_id],
                true
            );
            return;
        };

        // check that the hash of the received operations list matches the one in the header
        let computed_operations_hash = {
            let op_id_serializer = OperationIdSerializer::new();
            let op_ids = operation_ids
                .iter()
                .map(|op_id| {
                    let mut serialized = Vec::new();
                    op_id_serializer
                        .serialize(op_id, &mut serialized)
                        .expect("serialization of operation id should not fail");
                    serialized
                })
                .collect::<Vec<Vec<u8>>>();
            massa_hash::Hash::compute_from_tuple(
                &op_ids
                    .iter()
                    .map(|data| data.as_slice())
                    .collect::<Vec<_>>(),
            )
        };

        if wishlist_info
            .header
            .as_ref()
            .expect("header presence in wishlist should have been checked above")
            .content
            .operation_merkle_root
            != computed_operations_hash
        {
            warn!("Peer id {} sent us a operation list for block id {} but the hash in the header doesn't match.", from_peer_id, block_id);
            if let Err(err) = self.ban_peers(&[from_peer_id.clone()]) {
                warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
            }
            return;
        }

        // Mark the sender as knowing this block
        self.cache
            .write()
            .insert_peer_known_block(&from_peer_id, &[block_id], true);

        // Save the received operation ID list to the wishlist
        wishlist_info.operation_ids = Some(operation_ids);

        // free up all the nodes that we asked for that operation list
        self.remove_asked_blocks(&[block_id].into_iter().collect());
    }

    /// Return the sum of all operation's serialized sizes in the id list
    fn get_total_operations_size(storage: &Storage, operation_ids: &[OperationId]) -> usize {
        let op_read_lock = storage.read_operations();
        operation_ids
            .iter()
            .filter_map(|id| op_read_lock.get(id))
            .map(|op| op.serialized_size())
            .sum()
    }

    /// We received the full operations of a block.
    fn on_block_full_operations_received(
        &mut self,
        from_peer_id: PeerId,
        block_id: BlockId,
        operations: Vec<SecureShareOperation>,
    ) {
        // Ensure that we were looking for that data.
        let wishlist_info = if let Some(wishlist_info) = self.block_wishlist.get_mut(&block_id) && wishlist_info.header.is_some() && wishlist_info.operation_ids.is_some() {
            wishlist_info
        } else {
            // we were not looking for this data
            debug!("Peer id {} sent us full operations for block id {} but we were not looking for it", from_peer_id, block_id);
            // still mark the sender as knowing the block and operations
            self.cache.write().insert_peer_known_block(
                &from_peer_id,
                &[block_id],
                true
            );
            self.operation_cache.write().insert_peer_known_ops(
                &from_peer_id,
                &operations
                    .into_iter()
                    .map(|op| op.id.prefix())
                    .collect::<Vec<_>>(),
            );
            return;
        };

        // Move the ops into a hashmap
        let mut operations: PreHashMap<OperationId, SecureShareOperation> =
            operations.into_iter().map(|op| (op.id, op)).collect();

        // Make a set of all the block ops for fast lookup and deduplication
        let block_ops_set = wishlist_info
            .operation_ids
            .as_ref()
            .expect("operation_ids presence in wishlist should have been checked above")
            .iter()
            .copied()
            .collect::<PreHashSet<_>>();

        // claim the ops that we might have received in the meantime
        wishlist_info.storage.claim_operation_refs(&block_ops_set);

        {
            // filter out operations that we don't want or already know about
            let mut dropped_ops: PreHashSet<OperationId> = Default::default();
            operations.retain(|op_id, _| {
                if !block_ops_set.contains(op_id)
                    || wishlist_info.storage.get_op_refs().contains(op_id)
                {
                    dropped_ops.insert(*op_id);
                    return false;
                }
                true
            });

            // mark sender as knowing the dropped_ops
            self.operation_cache.write().insert_peer_known_ops(
                &from_peer_id,
                &dropped_ops
                    .into_iter()
                    .map(|op_id| op_id.prefix())
                    .collect::<Vec<_>>(),
            );
        }
        if operations.is_empty() {
            // we have most likely eliminated all the received operations in the filtering above
            return;
        }

        // Here we know that we were looking for that block's operations and that the sender node sent us some of the missing ones.

        // Check the validity of the received operations.
        // TODO: in the future if the validiy check fails for something non-malleable (eg. not sig verif),
        //       we should stop retrieving the block and ban everyone who knows it
        //       because we know for sure that this op's ID belongs to the block.
        if let Err(err) = note_operations_from_peer(
            &self.storage,
            &mut self.operation_cache,
            &self.config,
            operations.values().cloned().collect(),
            &from_peer_id,
            &mut self.sender_propagation_ops,
            &mut self.pool_controller,
        ) {
            warn!(
                "Peer id {} sent us operations for block id {} but they failed validity checks: {}",
                from_peer_id, block_id, err
            );
            if let Err(err) = self.ban_peers(&[from_peer_id.clone()]) {
                warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
            }
            return;
        }

        // add received operations to local storage and claim ref
        wishlist_info
            .storage
            .store_operations(operations.into_values().collect());

        if wishlist_info.storage.get_op_refs().len() == block_ops_set.len() {
            // if we gathered all the ops, we should delete the asked history and mark the sender as knowing the block
            self.remove_asked_blocks(&[block_id].into_iter().collect());

            // Mark the sender as knowing this block
            self.cache
                .write()
                .insert_peer_known_block(&from_peer_id, &[block_id], true);
        } else {
            // otherwise, we should remove the current peer ask only and mark it as not knowing the block
            // because it did not send us everything
            if let Some(asked) = self.asked_blocks.get_mut(&from_peer_id) {
                asked.remove(&block_id);
            }

            // Mark the sender as not knowing this block
            self.cache
                .write()
                .insert_peer_known_block(&from_peer_id, &[block_id], false);
        }
    }

    /// function that updates the global state of block retrieval
    pub(crate) fn update_block_retrieval(&mut self) {
        let ask_block_timeout = self.config.ask_block_timeout.to_duration();

        // Init timer for next tick
        let now = Instant::now();
        let mut next_tick = now
            .checked_add(self.config.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)
            .expect("could not compute next block retrieval timer tick");

        // Get conencted peer list
        let connected_peers = self.active_connections.get_peer_ids_connected();

        // Update cache
        self.cache.write().update_cache(&connected_peers);

        // Cleanup asked_blocks from all disconnected peers and blocks that are not in the wishlist anymore.
        self.asked_blocks.retain(|peer_id, asked_blocks| {
            if !connected_peers.contains(peer_id) {
                return false;
            }
            asked_blocks.retain(|block_id, _| self.block_wishlist.contains_key(block_id));
            !asked_blocks.is_empty()
        });

        // list of blocks that need to be asked
        let mut to_ask: PreHashSet<BlockId> = self.block_wishlist.keys().copied().collect();
        // the number of things already being asked to those peers
        let mut peer_loads: HashMap<PeerId, usize> = Default::default();
        for (peer_id, asked_blocks) in &mut self.asked_blocks {
            // init the list of items to remove from asked_blocks
            let mut to_remove_from_asked_blocks = Vec::new();
            for (block_id, ask_time) in asked_blocks.iter() {
                let expiry = ask_time
                    .checked_add(ask_block_timeout)
                    .expect("could not compute block ask expiry");
                if expiry <= now {
                    // the block has been asked for the block data a long time agp and did not respond

                    // we mark this peer as not knowing this block
                    self.cache
                        .write()
                        .insert_peer_known_block(peer_id, &[*block_id], false);

                    // We mark the block for removal from the asked_blocks list.
                    // This prevents us from re-detecting the timeout many times.
                    to_remove_from_asked_blocks.push(*block_id);
                } else {
                    // this block was recently asked to this peer: no need to ask for the block for now

                    to_ask.remove(block_id);

                    // mark this peer as loaded with an angoing ask
                    peer_loads
                        .entry(peer_id.clone())
                        .and_modify(|v| *v += 1)
                        .or_insert(1);

                    // update next tick
                    next_tick = next_tick.min(expiry);
                }
            }
            // remove the blocks marked for removal from asked_blocks
            for remove_id in to_remove_from_asked_blocks {
                asked_blocks.remove(&remove_id);
            }
        }

        // for each block to ask, choose a peer to ask it from and perform the ask
        let mut to_ask = to_ask.into_iter().collect::<Vec<_>>();
        to_ask.shuffle(&mut thread_rng()); // shuffle ask order
        for block_id in to_ask {
            // prioritize peers by (max knowledge, min knowledge age, min load, max random)
            let mut peer_scores: Vec<_> = connected_peers
                .iter()
                .filter_map(|peer_id| {
                    // Get the peer load. Look for the minimum score for asking.
                    let peer_load = peer_loads.get(peer_id).copied().unwrap_or_default();
                    if peer_load >= self.config.max_simultaneous_ask_blocks_per_node {
                        // this peer is already loaded with too many asks
                        return None;
                    }
                    // get peer knowledge info about that block
                    let peer_knowledge_of_block = self
                        .cache
                        .read()
                        .blocks_known_by_peer
                        .get(peer_id)
                        .and_then(|blocks_known| blocks_known.peek(&block_id).copied());
                    match peer_knowledge_of_block {
                        Some((false, info_t)) => {
                            // we think that the peer doesn't know the block
                            Some((
                                1i8,                                                               // worst knowledge
                                Some(-(now.saturating_duration_since(info_t).as_millis() as i64)), // the older the info the better
                                peer_load,                 // the lower the load the better
                                thread_rng().gen::<u64>(), // random tie breaker,
                                peer_id.clone(),
                            ))
                        }
                        None => {
                            // we don't know if the peer knows the block
                            Some((
                                0i8,                       // medium knowledge
                                None,                      // N/A
                                peer_load,                 // the lower the load the better
                                thread_rng().gen::<u64>(), // random tie breaker,
                                peer_id.clone(),
                            ))
                        }
                        Some((true, info_t)) => {
                            // we think that the peer knows the block
                            Some((
                                -1i8,                                                           // best knowledge
                                Some(now.saturating_duration_since(info_t).as_millis() as i64), // the newer the info the better
                                peer_load,                 // the lower the load the better
                                thread_rng().gen::<u64>(), // random tie breaker,
                                peer_id.clone(),
                            ))
                        }
                    }
                })
                .collect();

            // sort peers from best to worst to ask
            peer_scores.sort_unstable();

            // get wishlist info to deduce message to send
            let wishlist_info = self
                .block_wishlist
                .get_mut(&block_id)
                .expect("block presence in wishlist should have been checked above");
            let request = match (
                wishlist_info.header.is_some(),
                wishlist_info.operation_ids.is_some(),
            ) {
                // ask for header
                (false, false) => AskForBlockInfo::Header,
                // ask for the list of operation IDs in the block
                (true, false) => AskForBlockInfo::OperationIds,
                // ask for missing operations in the block
                (true, true) => {
                    // gather missing block operations and perform necessary followups
                    match self.gather_missing_block_ops(&block_id) {
                        Some(ops) => AskForBlockInfo::Operations(ops),
                        None => continue,
                    }
                }
                _ => panic!("invalid wishlist state"),
            };

            // try to ask peers from best to worst
            for (_, _, _, _, peer_id) in peer_scores {
                debug!("Send ask for block {} to {}", block_id, peer_id);
                if let Err(err) = self.active_connections.send_to_peer(
                    &peer_id,
                    &self.block_message_serializer,
                    Message::Block(Box::new(BlockMessage::DataRequest {
                        block_id,
                        block_info: request.clone(),
                    })),
                    true,
                ) {
                    warn!(
                        "Failed to send BlockDataRequest to peer {} err: {}",
                        peer_id, err
                    );
                } else {
                    // The request was sent.

                    // Update the asked_blocks list
                    self.asked_blocks
                        .entry(peer_id.clone())
                        .or_insert_with(Default::default)
                        .insert(block_id, now);

                    // Increment the load of the peer.
                    peer_loads
                        .entry(peer_id)
                        .and_modify(|v| *v += 1)
                        .or_insert(1);

                    // No need to look for other peers.
                    break;
                }
            }
        }

        // Update timer
        self.next_timer_ask_block = next_tick;
    }

    // Gather all missing block operations.
    // Returns Some(ops) if there are missing ops to gather
    fn gather_missing_block_ops(&mut self, block_id: &BlockId) -> Option<Vec<OperationId>> {
        // Get wishlist data
        let wishlist_info = match self.block_wishlist.get_mut(block_id) {
            // Wishlist data found
            Some(block_info) => {
                if block_info.header.is_none() || block_info.operation_ids.is_none() {
                    // Header or operation IDs not retrieved => cannot gather ops yet
                    return None;
                }
                block_info
            }
            // Wishlist data not found => nothing to do
            None => return None,
        };

        // Construct a hashset from the ID list for deduplication and faster lookup
        let op_id_list = wishlist_info
            .operation_ids
            .as_ref()
            .expect("operation IDs should be present");
        let op_id_set: PreHashSet<OperationId> = op_id_list.iter().copied().collect();

        // Gather all the ops in storage
        let claimed_ops = wishlist_info.storage.claim_operation_refs(&op_id_set);

        // Mark the ops we already know about as checked by us,
        // this is used to refresh our knowledge cache in case it had expired.
        if !claimed_ops.is_empty() {
            let mut cache_ops_write = self.operation_cache.write();
            for operation_id in claimed_ops.iter() {
                cache_ops_write.insert_checked_operation(*operation_id);
            }
        }

        // Compute the total operations size
        let total_operations_size = Self::get_total_operations_size(
            &wishlist_info.storage,
            &wishlist_info
                .operation_ids
                .as_ref()
                .expect("operation_ids presence in wishlist should have been checked above")
                .iter()
                .copied()
                .collect::<Vec<_>>(),
        );

        // Check if the total size of the operations we know about is greater than the max block size.
        // If it overflows, it means that the block is invalid because it is too big.
        // We should stop trying to retrieve the block and ban everyone who knows it.
        if total_operations_size > self.config.max_serialized_operations_size_per_block {
            warn!(
                "The operations we already have in our records exceed max block size for block {}.",
                block_id
            );

            // stop retrieving the block
            self.mark_block_as_invalid(&block_id);

            // quit
            return None;
        }

        // if there are missing blocks, return them
        if claimed_ops.len() < op_id_set.len() {
            return Some((&op_id_set - &claimed_ops).into_iter().collect());
        }

        // there are no missing ops, we can finish the block
        self.fully_gathered_block(&block_id);

        None
    }

    /// Called when we have fully gathered a block
    fn fully_gathered_block(&mut self, block_id: &BlockId) {
        // Gather all the elements needed to create the block. We must have it all by now.
        let wishlist_info = self
            .block_wishlist
            .remove(&block_id)
            .expect("block presence in wishlist should have been checked before");

        // Create the block
        let block = Block {
            header: wishlist_info
                .header
                .expect("header presence in wishlist should have been checked above"),
            operations: wishlist_info
                .operation_ids
                .expect("operation_ids presence in wishlist should have been checked above"),
        };

        let mut content_serialized = Vec::new();
        BlockSerializer::new() // todo : keep the serializer in the struct to avoid recreating it
            .serialize(&block, &mut content_serialized)
            .expect("failed to serialize block");

        // wrap block
        let signed_block = SecureShare {
            signature: block.header.signature.clone(),
            content_creator_pub_key: block.header.content_creator_pub_key.clone(),
            content_creator_address: block.header.content_creator_address.clone(),
            id: *block_id,
            content: block,
            serialized_data: content_serialized,
        };

        // Get block storage.
        // It should contain only the operations.
        let mut block_storage = wishlist_info.storage;

        // Add endorsements to storage and claim ref
        // TODO change this if we make endorsements separate from block header
        block_storage.store_endorsements(signed_block.content.header.content.endorsements.clone());

        // save slot
        let slot = signed_block.content.header.content.slot;

        // add block to storage and claim ref
        block_storage.store_block(signed_block);

        // Send to consensus
        self.consensus_controller
            .register_block(*block_id, slot, block_storage, false);

        // Remove from asked block history as it is not useful anymore
        self.remove_asked_blocks(&vec![*block_id].into_iter().collect());
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start_retrieval_thread(
    active_connections: Box<dyn ActiveConnectionsTrait>,
    selector_controller: Box<dyn SelectorController>,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    receiver_network: MassaReceiver<PeerMessageTuple>,
    receiver: MassaReceiver<BlockHandlerRetrievalCommand>,
    _internal_sender: MassaSender<BlockHandlerPropagationCommand>,
    sender_propagation_ops: MassaSender<OperationHandlerPropagationCommand>,
    sender_propagation_endorsements: MassaSender<EndorsementHandlerPropagationCommand>,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    config: ProtocolConfig,
    endorsement_cache: SharedEndorsementCache,
    operation_cache: SharedOperationCache,
    cache: SharedBlockCache,
    storage: Storage,
    mip_store: MipStore,
    massa_metrics: MassaMetrics,
) -> JoinHandle<()> {
    let block_message_serializer =
        MessagesSerializer::new().with_block_message_serializer(BlockMessageSerializer::new());
    std::thread::Builder::new()
        .name("protocol-block-handler-retrieval".to_string())
        .spawn(move || {
            let mut retrieval_thread = RetrievalThread {
                active_connections,
                selector_controller,
                consensus_controller,
                pool_controller,
                next_timer_ask_block: Instant::now() + config.ask_block_timeout.to_duration(),
                block_wishlist: PreHashMap::default(),
                asked_blocks: HashMap::default(),
                peer_cmd_sender,
                sender_propagation_ops,
                sender_propagation_endorsements,
                receiver_network,
                block_message_serializer,
                receiver,
                _announcement_sender: _internal_sender,
                cache,
                endorsement_cache,
                operation_cache,
                config,
                storage,
                mip_store,
                massa_metrics,
            };
            retrieval_thread.run();
        })
        .expect("OS failed to start block retrieval thread")
}
