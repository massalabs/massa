use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    thread::JoinHandle,
    time::Instant,
};

use crate::{
    handlers::{
        endorsement_handler::{
            cache::SharedEndorsementCache,
            commands_propagation::EndorsementHandlerPropagationCommand,
        },
        operation_handler::{
            cache::SharedOperationCache, commands_propagation::OperationHandlerPropagationCommand,
        },
        peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
    },
    messages::MessagesSerializer,
    sig_verifier::verify_sigs_batch,
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
    endorsement::SecureShareEndorsement,
    operation::{OperationId, SecureShareOperation},
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    secure_share::{Id, SecureShare},
    slot::Slot,
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_storage::Storage;
use massa_time::{MassaTime, TimeError};
use massa_versioning::versioning::MipStore;
use schnellru::{ByLength, LruMap};
use tracing::{debug, info, warn};

use super::{
    cache::SharedBlockCache,
    commands_propagation::BlockHandlerPropagationCommand,
    commands_retrieval::BlockHandlerRetrievalCommand,
    messages::{
        AskForBlockInfo, BlockInfoReply, BlockMessage, BlockMessageDeserializer,
        BlockMessageDeserializerArgs,
    },
    BlockMessageSerializer,
};

static BLOCK_HEADER: &str = "protocol.protocol_worker.on_network_event.received_block_header";

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
            operations_size: 0,
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
                                BlockMessage::BlockDataRequest{block_id, block_info} => {
                                    self.on_ask_for_block_info_received(peer_id.clone(), block_id, block_info);
                                }
                                BlockMessage::BlockDataResponse{block_id, block_info} => {
                                   self.on_block_info_received(peer_id.clone(), block_id, block_info);
                                   self.update_block_retrieval();
                                }
                                BlockMessage::BlockHeader(header) => {
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
    /// We send the block's operation ids if the foreign node asked for `AskForBlocksInfo::Info`
    /// or a subset of the full operations of the block if it asked for `AskForBlocksInfo::Operations`.
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
                endorsement_knowledge_updates
                    .extend(header.content.endorsements.iter().map(|e| e.id).collect());

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
                operation_knowledge_updates.extend(returned_ops.iter().map(|op| op.id).collect());

                BlockInfoReply::Operations(returned_ops)
            }
        };

        debug!("Send reply for block info to {}", from_peer_id);

        // send response to peer
        if let Err(err) = self.active_connections.send_to_peer(
            &from_peer_id,
            &self.block_message_serializer,
            BlockMessage::BlockDataResponse {
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
            self.cache.write().insert_blocks_known(
                &from_peer_id,
                &block_knowledge_updates.into_iter().collect::<Vec<_>>(),
                true,
            );
        }
        if !operation_knowledge_updates.is_empty() {
            self.operation_cache.write().insert_ops_known(
                &from_peer_id,
                &operation_knowledge_updates.into_iter().collect::<Vec<_>>(),
            );
        }
        if !endorsement_knowledge_updates.is_empty() {
            self.endorsement_cache.write().insert_endorsements_known(
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
                    .insert_blocks_known(&from_peer_id, &[block_id], false);
            }
        }
    }

    /// On block header received from a node.
    fn on_block_header_received(
        &mut self,
        from_peer_id: PeerId,
        header: SecuredHeader,
    ) {
        let block_id = header.id;

        // Check header and update knowledge info
        let is_new = match self.note_header_from_peer(&header, &from_peer_id) {
            Ok(is_new) => is_new,
            Err(err) => {
                warn!(
                    "peer {} sent us critically incorrect header: {}",
                    from_peer_id, err
                );
                if let Err(err) = self.ban_node(&from_peer_id) {
                    warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
                }
                return;
            }
        };

        if let Some(info) = self.block_wishlist.get_mut(&block_id) {
            // if the header is in our wishlist
            if info.header.is_none() {
                // if we were looking for the missing header

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
            return Err(ProtocolError::InvalidBlock(format!("block is genesis")));
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
                cache_write.insert_blocks_known(
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
                let endorsement_ids = header.content.endorsements.iter().map(|e| e.id).collect();
                self.endorsement_cache
                    .write()
                    .insert_known_endorsements(&from_peer_id, &endorsement_ids);
            }

            // mark the sender peer as knowing the operations of the block (if we know them)
            let opt_block_ops = self
                .storage
                .read_blocks()
                .get(&block_id)
                .map(|b| b.content.operations.clone());
            if let Some(block_ops) = opt_block_ops {
                self.operation_cache
                    .write()
                    .insert_known_operations(&from_peer_id, &block_ops);
            }

            // return that we already know that header
            return Ok(false);
        }

        // check endorsements
        if let Err(err) =
            self.note_endorsements_from_peer(header.content.endorsements.clone(), from_peer_id)
        {
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
            let endorsement_ids = header.content.endorsements.iter().map(|e| e.id).collect();
            self.endorsement_cache
                .write()
                .insert_known_endorsements(&from_peer_id, &endorsement_ids);
        }

        {
            let cache_lock = self.cache.write();

            // mark the sender peer as knowing the block and its parents
            self.cache.write().insert_blocks_known(
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
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(vec![peer_id.clone()]))
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

    /// Note endorsements coming from a given node,
    /// and propagate them when they were received outside of a header.
    ///
    /// Caches knowledge of valid ones.
    ///
    /// Does not ban if the endorsement is invalid
    ///
    /// Checks performed:
    /// - Valid signature.
    pub(crate) fn note_endorsements_from_peer(
        &mut self,
        endorsements: Vec<SecureShareEndorsement>,
        from_peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
        todo_refactor_endorsmeent_system_and_align_this(); // SHARE SAME FUNC
        massa_trace!("protocol.protocol_worker.note_endorsements_from_node", { "node": from_peer_id, "endorsements": endorsements});
        let length = endorsements.len();
        let mut new_endorsements = PreHashMap::with_capacity(length);
        let mut endorsement_ids = PreHashSet::with_capacity(length);
        for endorsement in endorsements.into_iter() {
            let endorsement_id = endorsement.id;
            endorsement_ids.insert(endorsement_id);
            // check endorsement signature if not already checked
            {
                let read_cache = self.endorsement_cache.read();
                if read_cache
                    .checked_endorsements
                    .peek(&endorsement_id)
                    .is_none()
                {
                    new_endorsements.insert(endorsement_id, endorsement);
                }
            }
        }

        // Batch signature verification
        // optimized signature verification
        verify_sigs_batch(
            &new_endorsements
                .values()
                .map(|endorsement| {
                    (
                        endorsement.compute_signed_hash(),
                        endorsement.signature,
                        endorsement.content_creator_pub_key,
                    )
                })
                .collect::<Vec<_>>(),
        )?;

        // Check PoS draws
        for endorsement in new_endorsements.values() {
            let selection = self
                .selector_controller
                .get_selection(endorsement.content.slot)?;
            let Some(address) = selection.endorsements.get(endorsement.content.index as usize) else {
                return Err(ProtocolError::GeneralProtocolError(
                    format!(
                        "No selection on slot {} for index {}",
                        endorsement.content.slot, endorsement.content.index
                    )
                ))
            };
            if address != &endorsement.content_creator_address {
                return Err(ProtocolError::GeneralProtocolError(format!(
                    "Invalid endorsement: expected address {}, got {}",
                    address, endorsement.content_creator_address
                )));
            }
        }

        'write_cache: {
            let mut cache_write = self.endorsement_cache.write();
            // add to verified signature cache
            for endorsement_id in endorsement_ids.iter() {
                cache_write.checked_endorsements.insert(*endorsement_id, ());
            }
            // add to known endorsements for source node.
            let Ok(endorsements) = cache_write
                .endorsements_known_by_peer
                .get_or_insert(from_peer_id.clone(), || {
                    LruMap::new(ByLength::new(
                        self.config
                            .max_node_known_endorsements_size
                            .try_into()
                            .expect("max_node_known_endorsements_size in config should be > 0"),
                    ))
                })
                .ok_or(()) else {
                    warn!("endorsements_known_by_peer limit reached");
                    break 'write_cache;
                };
            for endorsement_id in endorsement_ids.iter() {
                endorsements.insert(*endorsement_id, ());
            }
        }

        if !new_endorsements.is_empty() {
            let mut endorsements = self.storage.clone_without_refs();
            endorsements.store_endorsements(new_endorsements.into_values().collect());

            // Propagate endorsements
            // Propagate endorsements when the slot of the block they endorse isn't `max_endorsements_propagation_time` old.
            let mut endorsements_to_propagate = endorsements.clone();
            let endorsements_to_not_propagate = {
                let now = MassaTime::now()?;
                let read_endorsements = endorsements_to_propagate.read_endorsements();
                endorsements_to_propagate
                    .get_endorsement_refs()
                    .iter()
                    .filter_map(|endorsement_id| {
                        let slot_endorsed_block =
                            read_endorsements.get(endorsement_id).unwrap().content.slot;
                        let slot_timestamp = get_block_slot_timestamp(
                            self.config.thread_count,
                            self.config.t0,
                            self.config.genesis_timestamp,
                            slot_endorsed_block,
                        );
                        match slot_timestamp {
                            Ok(slot_timestamp) => {
                                if slot_timestamp
                                    .saturating_add(self.config.max_endorsements_propagation_time)
                                    < now
                                {
                                    Some(*endorsement_id)
                                } else {
                                    None
                                }
                            }
                            Err(_) => Some(*endorsement_id),
                        }
                    })
                    .collect()
            };
            endorsements_to_propagate.drop_endorsement_refs(&endorsements_to_not_propagate);
            if let Err(err) = self.sender_propagation_endorsements.try_send(
                EndorsementHandlerPropagationCommand::PropagateEndorsements(
                    endorsements_to_propagate,
                ),
            ) {
                warn!("Failed to send from block retrieval thread of endorsement handler to propagation: {:?}", err);
            }
            // Add to pool
            self.pool_controller.add_endorsements(endorsements);
        }

        Ok(())
    }

    /// Mark a block as invalid
    fn mark_block_as_invalid(&mut self, block_id: &BlockId) {
            // stop retrieving the block
            if let Some(wishlist_info) = self.block_wishlist.remove(&block_id) {
                if let Some(header) = wishlist_info.header {
                    // notify consensus that the block is invalid
                    self.consensus_controller.mark_invalid_block(*block_id, header);
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
        self.operation_cache.write().insert_known_operations(
            &from_peer_id,
            &operation_ids_set
                .iter()
                .map(|op_id| op_id.prefix())
                .collect(),
        );

        // check if we were looking to retrieve the list of ops for that block
        let wishlist_info = if let Some(info) = self.block_wishlist.get_mut(&block_id) && info.header.is_some() && info.operation_ids.is_none() {
            // we were actively looking for this data
            info
        } else {
            // we were not actively looking for that data, but mark the remote node as knowing the block and listed ops
            debug!("peer {} sent us a list of operation IDs for block id {} but we were not looking for it", from_peer_id, block_id);
            todo_mark_remote_as_knowing_block_and_ops_and_possibly_header_and_endorsements();
            return;
        };

        // check that the hash of the received operations list matches the one in the header
        let computed_operations_hash = massa_hash::Hash::compute_from_tuple(
            &operation_ids
                .iter()
                .map(|op_id| &op_id.to_bytes()[..])
                .collect::<Vec<_>>(),
        );
        if wishlist_info
            .header
            .expect("header presence in wishlist should have been checked above")
            .content
            .operation_hash
            != computed_operations_hash
        {
            warn!("Peer id {} sent us a operation list for block id {} but the hash in the header doesn't match.", from_peer_id, block_id);
            if let Err(err) = self.ban_node(&from_peer_id) {
                warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
            }
            return;
        }

        // Save the received operation ID list to the wishlist
        wishlist_info.operation_ids = Some(operation_ids.clone());

        // claim the operations we already know about from that list
        let known_operations = wishlist_info
            .storage
            .claim_operation_refs(&operation_ids_set);

        // Mark the ops we already know about as checked by us,
        // this is used to refresh our knowledge cache in case it had expired
        if !known_operations.is_empty() {
            let mut cache_ops_write = self.operation_cache.write();
            for operation_id in known_operations.iter() {
                cache_ops_write.insert_checked_operation(*operation_id);
            }
        }

        // get the total size of known ops
        // note that if the same op appears twice, it needs to be counted twice
        let total_operations_size = Self::get_total_operations_size(
            &self.storage,
            &operation_ids
                .iter()
                .filter(|id| known_operations.contains(id))
                .copied()
                .collect::<Vec<_>>(),
        );

        // Check if the total size of the operations we know about is greater than the max block size.
        // If it overflows, it means that the block is invalid because it is too big.
        // We should stop trying to retrieve the block and ban everyone who knows it.
        if total_operations_size > self.config.max_serialized_operations_size_per_block {
            warn!("Peer id {} sent us a operation list for block id {} but the operations we already have in our records exceed max block size.", from_peer_id, block_id);

            // stop retrieving the block
            self.mark_block_as_invalid(&block_id);

            // ban the sender node because they were not supposed to propagate an invalid block
            if let Err(err) = self.ban_node(&from_peer_id) {
                warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
            }

            // quit
            return;
        }

        // free up all the nodes that we asked for that operation list
        self.remove_asked_blocks(&[block_id].into_iter().collect());

        // If the block is empty, go straight to processing the full block info.
        if operation_ids.is_empty() {
            self.on_block_full_operations_received(
                from_peer_id,
                block_id,
                Default::default(),
            );
        }
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
            debug!("Peer id {} sent us full operations for block id {} but we were not looking for it", from_peer_id, block_id);
            todo_mark_remote_as_knowing_block_and_ops_and_possibly_header_and_endorsements();
            return;
        };

        // Move the ops into a hashmap
        let mut operations: PreHashMap<OperationId, SecureShareOperation> =
            operations.into_iter().map(|op| (op.id, op)).collect();
        
        // Make a set of all the block ops for fast lookup ande deduplication
        let block_ops_set = wishlist_info
            .operation_ids
            .as_ref()
            .expect("operation_ids presence in wishlist should have been checked above")
            .iter()
            .copied()
            .collect::<PreHashSet<_>>();

        // just in case, claim the ops that we might have received in the meantime
        wishlist_info.storage.claim_operation_refs(&block_ops_set);

        {
            // filter out operations that we don't want or already know about
            let mut dropped_ops: PreHashSet<OperationId> = Default::default();
            operations.retain(|op_id, _| {
                if !block_ops_set.contains(op_id) || !wishlist_info.storage.get_op_refs().contains(op_id) {
                    dropped_ops.insert(*op_id);
                    return false;
                }
                true
            });

            if operations.is_empty() {
                // we have most likely eliminated all the received operations in the filtering above
                return;
            }

            // mark sender as knowing the dropped_ops
            self.operation_cache.write().insert_known_operations(
                &from_peer_id,
                &dropped_ops.into_iter().map(|op_id| op_id.prefix()).collect(),
            );
        }

        // Here we know that we were looking for that block's operations and that the sender node sent us some of the missing ones.
    
        // Check the validity of the received operations.
        // TODO: in the future if the validiy check fails for something non-malleable (eg. not sig verif),
        //       we should stop retrieving the block and ban everyone who knows it
        //       because we know for sure that this op's ID belongs to the block.
        if let Err(err) = self.note_operations_from_peer(operations.values().cloned().collect(), &from_peer_id) {
            warn!(
                "Peer id {} sent us operations for block id {} but they failed validity checks: {}",
                from_peer_id, block_id, err
            );
            if let Err(err) = self.ban_node(&from_peer_id) {
                warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
            }
            return;
        }

        // add received operations to local storage and claim ref
        wishlist_info.storage.store_operations(operations.into_values().collect());

        // Compute the total operations size
        let total_operations_size = Self::get_total_operations_size(
                &wishlist_info.storage,
                &wishlist_info
                    .operation_ids
                    .as_ref()
                    .expect("operation_ids presence in wishlist should have been checked above")
                    .iter()
                    .copied()
                    .collect::<Vec<_>>()
        );

        // Check if the total size of the operations we know about is greater than the max block size.
        // If it overflows, it means that the block is invalid because it is too big.
        // We should stop trying to retrieve the block and ban everyone who knows it.
        if total_operations_size > self.config.max_serialized_operations_size_per_block {
            warn!("Peer id {} sent us a operation list for block id {} but the operations we already have in our records exceed max block size.", from_peer_id, block_id);

            // stop retrieving the block
            self.mark_block_as_invalid(&block_id);

            // ban the sender node because they were not supposed to propagate an invalid block
            if let Err(err) = self.ban_node(&from_peer_id) {
                warn!("Error while banning peer {} err: {:?}", from_peer_id, err);
            }

            // quit
            return;
        }

        // Check whether we have received all the operations for that block.
        if wishlist_info.storage.get_op_refs().len() < block_ops_set.len() {
            // we are still missing some operations.
            // Note that we don't mark the sender as knowing the block here
            // because they failed to send us all the ops we needed.
            // This can happen if the sender node is malicious or if they dropped the block mid-process.
            return;
        }

        // Gather all the elemnents needed to create the block. We must have it all by now.
        let wishlist_info = self.block_wishlist.remove(&block_id).expect("block presence in wishlist should have been checked above");

        // Create the block
        let block = Block {
            header: wishlist_info.header.expect("header presence in wishlist should have been checked above"),
            operations: wishlist_info.operation_ids.expect("operation_ids presence in wishlist should have been checked above"),
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
            id: block_id,
            content: block,
            serialized_data: content_serialized,
        };

        // Get block storage.
        // It should contain only the operations.
        let mut block_storage = wishlist_info.storage;

        // Add endorsements to storage and claim ref
        // TODO change this if we make endorsements separate from block header
        block_storage.store_endorsements(
            signed_block.content.header.content.endorsements.clone(),
        );

        // save slot
        let slot = signed_block.content.header.content.slot;

        // add block to storage and claim ref
        block_storage.store_block(signed_block);

        // Send to consensus
        self.consensus_controller
            .register_block(block_id, slot, block_storage, false);
        
        // Update ask block
        self.remove_asked_blocks(&vec![block_id].into_iter().collect());
    }

    RESUME FROM HERE

    fn note_operations_from_peer(
        &mut self,
        operations: Vec<SecureShareOperation>,
        source_peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
        use_a_common_function_with_operation_handler(); // SHARE SAME FUNC

        massa_trace!("protocol.protocol_worker.note_operations_from_peer", { "peer": source_peer_id, "operations": operations });
        let now = MassaTime::now().expect("could not get current time");

        let mut new_operations = PreHashMap::with_capacity(operations.len());
        for operation in operations {
            // ignore if op is too old
            let expire_period_timestamp = get_block_slot_timestamp(
                self.config.thread_count,
                self.config.t0,
                self.config.genesis_timestamp,
                Slot::new(
                    operation.content.expire_period,
                    operation
                        .content_creator_address
                        .get_thread(self.config.thread_count),
                ),
            );
            match expire_period_timestamp {
                Ok(slot_timestamp) => {
                    if slot_timestamp.saturating_add(self.config.max_operations_propagation_time)
                        < now
                    {
                        continue;
                    }
                }
                Err(_) => continue,
            }

            // quit if op is too big
            if operation.serialized_size() > self.config.max_serialized_operations_size_per_block {
                return Err(ProtocolError::InvalidOperationError(format!(
                    "Operation {} exceeds max block size,  maximum authorized {} bytes but found {} bytes",
                    operation.id,
                    operation.serialized_size(),
                    self.config.max_serialized_operations_size_per_block
                )));
            };

            // add to new operations
            new_operations.insert(operation.id, operation);
        }

        // all valid received ids (not only new ones) for knowledge marking
        let all_received_ids: PreHashSet<_> = new_operations.keys().copied().collect();

        // retain only new ops that are not already known
        {
            let cache_read = self.operation_cache.read();
            new_operations.retain(|op_id, _| cache_read.checked_operations.peek(op_id).is_none());
        }

        // optimized signature verification
        verify_sigs_batch(
            &new_operations
                .iter()
                .map(|(op_id, op)| (*op_id.get_hash(), op.signature, op.content_creator_pub_key))
                .collect::<Vec<_>>(),
        )?;

        {
            // add to checked operations
            let mut cache_write = self.operation_cache.write();

            // add checked operations
            for op_id in new_operations.keys().copied() {
                cache_write.insert_checked_operation(op_id);
            }

            // add to known ops
            let known_ops = cache_write
                .ops_known_by_peer
                .entry(source_peer_id.clone())
                .or_insert_with(|| {
                    LruMap::new(ByLength::new(
                        self.config
                            .max_node_known_ops_size
                            .try_into()
                            .expect("max_node_known_ops_size in config must be > 0"),
                    ))
                });
            for id in all_received_ids {
                known_ops.insert(id.prefix(), ());
            }
        }

        if !new_operations.is_empty() {
            // Store new operations, claim locally
            let mut ops = self.storage.clone_without_refs();
            ops.store_operations(new_operations.into_values().collect());

            self.sender_propagation_ops
                .try_send(OperationHandlerPropagationCommand::PropagateOperations(
                    ops.clone(),
                ))
                .map_err(|err| ProtocolError::SendError(err.to_string()))?;

            // Add to pool
            self.pool_controller.add_operations(ops);
        }

        Ok(())
    }

    pub(crate) fn update_block_retrieval(&mut self) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.update_ask_block.begin", {});
        let now = Instant::now();

        // init timer
        let mut next_tick = now
            .checked_add(self.config.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;

        // list blocks to re-ask and gather candidate nodes to ask from
        let mut candidate_nodes: PreHashMap<BlockId, Vec<_>> = Default::default();
        let mut ask_block_list: HashMap<PeerId, Vec<(BlockId, AskForBlockInfo)>> =
            Default::default();

        // list blocks to re-ask and from whom
        {
            let mut cache_write = self.cache.write();
            for (hash, block_info) in self.block_wishlist.iter() {
                let required_info = if block_info.header.is_none() {
                    AskForBlockInfo::Header
                } else if block_info.operation_ids.is_none() {
                    AskForBlockInfo::OperationIds
                } else {
                    let already_stored_operations = block_info.storage.get_op_refs();
                    // Unwrap safety: Check if `operation_ids` is none just above
                    AskForBlockInfo::Operations(
                        block_info
                            .operation_ids
                            .as_ref()
                            .unwrap()
                            .iter()
                            .filter(|id| !already_stored_operations.contains(id))
                            .copied()
                            .collect(),
                    )
                };
                let mut needs_ask = true;

                let peers_connected = self.active_connections.get_peer_ids_connected();
                cache_write.update_cache(&peers_connected);
                let peers_in_asked_blocks: Vec<PeerId> =
                    self.asked_blocks.keys().cloned().collect();
                for peer_id in peers_in_asked_blocks {
                    if !peers_connected.contains(&peer_id) {
                        self.asked_blocks.remove(&peer_id);
                    }
                }
                for peer_id in peers_connected {
                    if !self.asked_blocks.contains_key(&peer_id) {
                        self.asked_blocks
                            .insert(peer_id.clone(), PreHashMap::default());
                    }
                }
                let all_keys: Vec<PeerId> = cache_write
                    .blocks_known_by_peer
                    .iter()
                    .map(|(k, _)| k)
                    .cloned()
                    .collect();
                for peer_id in all_keys.iter() {
                    // for (peer_id, (blocks_known, _)) in cache_write.blocks_known_by_peer.iter() {
                    let blocks_known = cache_write.blocks_known_by_peer.get_mut(peer_id).unwrap();
                    // map to remove the borrow on asked_blocks. Otherwise can't call insert_known_blocks
                    let ask_time_opt = self
                        .asked_blocks
                        .get(peer_id)
                        .and_then(|asked_blocks| asked_blocks.get(hash).copied());
                    let (timeout_at_opt, timed_out) = if let Some(ask_time) = ask_time_opt {
                        let t = ask_time
                            .checked_add(self.config.ask_block_timeout.into())
                            .ok_or(TimeError::TimeOverflowError)?;
                        (Some(t), t <= now)
                    } else {
                        (None, false)
                    };
                    let knows_block = blocks_known.get(hash);

                    // check if the peer recently told us it doesn't have the block
                    if let Some((false, info_time)) = knows_block {
                        let info_expires = info_time
                            .checked_add(self.config.ask_block_timeout.into())
                            .ok_or(TimeError::TimeOverflowError)?;
                        if info_expires > now {
                            next_tick = std::cmp::min(next_tick, info_expires);
                            continue; // ignore candidate peer
                        }
                    }

                    let candidate = match (timed_out, timeout_at_opt, knows_block) {
                        // not asked yet
                        (_, None, knowledge) => match knowledge {
                            Some((true, _)) => (0u8, None),
                            None => (1u8, None),
                            Some((false, _)) => (2u8, None),
                        },
                        // not timed out yet (note: recent DONTHAVBLOCK checked before the match)
                        (false, Some(timeout_at), _) => {
                            next_tick = std::cmp::min(next_tick, timeout_at);
                            needs_ask = false; // no need to re ask
                            continue; // not a candidate
                        }
                        // timed out, supposed to have it
                        (true, Some(mut timeout_at), Some((true, info_time))) => {
                            if info_time < &mut timeout_at {
                                // info less recent than timeout: mark as not having it
                                blocks_known.insert(*hash, (false, timeout_at));
                                (2u8, ask_time_opt)
                            } else {
                                // told us it has it after a timeout: good candidate again
                                (0u8, ask_time_opt)
                            }
                        }
                        // timed out, supposed to not have it
                        (true, Some(mut timeout_at), Some((false, info_time))) => {
                            if info_time < &mut timeout_at {
                                // info less recent than timeout: update info time
                                blocks_known.insert(*hash, (false, timeout_at));
                            }
                            (2u8, ask_time_opt)
                        }
                        // timed out but don't know if has it: mark as not having it
                        (true, Some(timeout_at), None) => {
                            blocks_known.insert(*hash, (false, timeout_at));
                            (2u8, ask_time_opt)
                        }
                    };

                    // add candidate peer
                    candidate_nodes.entry(*hash).or_insert_with(Vec::new).push((
                        candidate,
                        peer_id.clone(),
                        required_info.clone(),
                    ));
                }

                // remove if doesn't need to be asked
                if !needs_ask {
                    candidate_nodes.remove(hash);
                }
            }
        }

        // count active block requests per node
        let mut active_block_req_count: HashMap<PeerId, usize> = self
            .asked_blocks
            .iter()
            .map(|(peer_id, blocks)| {
                (
                    peer_id.clone(),
                    blocks
                        .iter()
                        .filter(|(_h, ask_t)| {
                            ask_t
                                .checked_add(self.config.ask_block_timeout.into())
                                .map_or(false, |timeout_t| timeout_t > now)
                        })
                        .count(),
                )
            })
            .collect();
        {
            let cache_read = self.cache.read();
            for (hash, criteria) in candidate_nodes.into_iter() {
                // find the best node
                if let Some((_knowledge, best_node, required_info, _)) = criteria
                    .into_iter()
                    .filter_map(|(knowledge, peer_id, required_info)| {
                        // filter out nodes with too many active block requests
                        if *active_block_req_count.get(&peer_id).unwrap_or(&0)
                            <= self.config.max_simultaneous_ask_blocks_per_node
                        {
                            cache_read
                                .blocks_known_by_peer
                                .get(&peer_id)
                                .map(|peer_data| (knowledge, peer_id, required_info, peer_data.1))
                        } else {
                            None
                        }
                    })
                    .min_by_key(|(knowledge, peer_id, _, instant)| {
                        (
                            *knowledge,                                         // block knowledge
                            *active_block_req_count.get(peer_id).unwrap_or(&0), // active requests
                            *instant,                                           // node age
                            peer_id.clone(),                                    // node ID
                        )
                    })
                {
                    let asked_blocks = self.asked_blocks.get_mut(&best_node).unwrap(); // will not panic, already checked
                    asked_blocks.insert(hash, now);
                    if let Some(cnt) = active_block_req_count.get_mut(&best_node) {
                        *cnt += 1; // increase the number of actively asked blocks
                    }

                    ask_block_list
                        .entry(best_node.clone())
                        .or_insert_with(Vec::new)
                        .push((hash, required_info.clone()));

                    let timeout_at = now
                        .checked_add(self.config.ask_block_timeout.into())
                        .ok_or(TimeError::TimeOverflowError)?;
                    next_tick = std::cmp::min(next_tick, timeout_at);
                }
            }
        }

        // send AskBlockEvents
        if !ask_block_list.is_empty() {
            for (peer_id, list) in ask_block_list.iter() {
                for sub_list in list.chunks(self.config.max_size_block_infos as usize) {
                    debug!("Send ask for blocks of len {} to {}", list.len(), peer_id);
                    if let Err(err) = self.active_connections.send_to_peer(
                        peer_id,
                        &self.block_message_serializer,
                        BlockMessage::BlockDataRequest(sub_list.to_vec()).into(),
                        true,
                    ) {
                        warn!(
                            "Failed to send AskForBlocks to peer {} err: {}",
                            peer_id, err
                        );
                    }
                }
            }
        }

        self.next_timer_ask_block = next_tick;
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
// bookmark
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
