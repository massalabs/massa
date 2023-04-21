use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    thread::JoinHandle,
    time::Instant,
};

use crate::{
    handlers::{
        endorsement_handler::cache::SharedEndorsementCache,
        peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
    },
    messages::MessagesSerializer,
    sig_verifier::verify_sigs_batch,
};
use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use lru::LruCache;
use massa_consensus_exports::ConsensusController;
use massa_logging::massa_trace;
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    endorsement::SecureShareEndorsement,
    operation::OperationId,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    secure_share::Id,
};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};
use tracing::warn;

use super::{
    cache::SharedBlockCache,
    commands_propagation::BlockHandlerCommand,
    commands_retrieval::BlockHandlerRetrievalCommand,
    messages::{
        AskForBlocksInfo, BlockInfoReply, BlockMessage, BlockMessageDeserializer,
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
    /// Full operations size in bytes
    pub(crate) operations_size: usize,
}

pub struct RetrievalThread {
    active_connections: SharedActiveConnections,
    consensus_controller: Box<dyn ConsensusController>,
    pool_controller: Box<dyn PoolController>,
    receiver_network: Receiver<PeerMessageTuple>,
    internal_sender: Sender<BlockHandlerCommand>,
    receiver: Receiver<BlockHandlerRetrievalCommand>,
    block_message_serializer: MessagesSerializer,
    block_wishlist: PreHashMap<BlockId, BlockInfo>,
    asked_blocks: HashMap<PeerId, PreHashMap<BlockId, Instant>>,
    peer_cmd_sender: Sender<PeerManagementCmd>,
    endorsement_cache: SharedEndorsementCache,
    cache: SharedBlockCache,
    config: ProtocolConfig,
    storage: Storage,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Add real values
        let mut block_message_deserializer =
            BlockMessageDeserializer::new(BlockMessageDeserializerArgs {
                thread_count: 32,
                endorsement_count: 10000,
                block_infos_length_max: 10000,
                max_operations_per_block: 10000,
                max_datastore_value_length: 10000,
                max_function_name_length: 10000,
                max_parameters_size: 10000,
                max_op_datastore_entry_count: 10000,
                max_op_datastore_key_length: 100,
                max_op_datastore_value_length: 10000,
            });
        loop {
            select! {
                recv(self.receiver_network) -> msg => {
                    match msg {
                        Ok((peer_id, message_id, message)) => {
                            block_message_deserializer.set_message_id(message_id);
                            let (rest, message) = block_message_deserializer
                                .deserialize::<DeserializeError>(&message)
                                .unwrap();
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                BlockMessage::AskForBlocks(block_infos) => {
                                    if let Err(err) = self.on_asked_for_blocks_received(peer_id, block_infos) {
                                        warn!("Error in on_asked_for_blocks_received: {:?}", err);
                                    }
                                }
                                BlockMessage::ReplyForBlocks(block_infos) => {
                                    for (block_id, block_info) in block_infos.into_iter() {
                                        if let Err(err) = self.on_block_info_received(peer_id, block_id, block_info) {
                                            warn!("Error in on_block_info_received: {:?}", err);
                                        }
                                    }
                                    //TODO: Block algorithm
                                }
                                BlockMessage::BlockHeader(header) => {
                                    massa_trace!(BLOCK_HEADER, { "peer_id": peer_id, "header": header});
                                    if let Ok(Some((block_id, is_new))) =
                                        self.note_header_from_peer(&header, &peer_id)
                                    {
                                        if is_new {
                                            self.consensus_controller
                                                .register_block_header(block_id, header);
                                        }
                                        //TODO: Block algorithm
                                        //self.update_ask_block(block_ask_timer).await?;
                                    } else {
                                        warn!(
                                            "peer {} sent us critically incorrect header, \
                                            which may be an attack attempt by the remote peer \
                                            or a loss of sync between us and the remote peer",
                                            peer_id,
                                        );
                                        let _ = self.ban_node(&peer_id);
                                    }
                                }
                            }
                        },
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                },
                recv(self.receiver) -> msg => {
                    match msg {
                        Ok(command) => {
                        },
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Network ask the local node for blocks
    ///
    /// React on another node asking for blocks information. We can forward the operation ids if
    /// the foreign node asked for `AskForBlocksInfo::Info` or the full operations if he asked for
    /// the missing operations in his storage with `AskForBlocksInfo::Operations`
    ///
    /// Forward the reply to the network.
    fn on_asked_for_blocks_received(
        &mut self,
        from_peer_id: PeerId,
        list: Vec<(BlockId, AskForBlocksInfo)>,
    ) -> Result<(), ProtocolError> {
        // let node_info = match self.active_nodes.get_mut(&from_node_id) {
        //     Some(node_info) => node_info,
        //     _ => return Ok(()),
        // };
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
                    {
                        let mut cache_write = self.cache.write();
                        let known_blocks = cache_write.blocks_known_by_peer.get_or_insert_mut(
                            from_peer_id,
                            || {
                                LruCache::new(
                                    NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                        .expect("max_node_known_blocks_size in config must be > 0"),
                                )
                            },
                        );
                        known_blocks.put(*hash, ());
                    }
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
        // Clean shared cache if peers do not exist anymore
        {
            let mut cache_write = self.cache.write();
            let peers: Vec<PeerId> = cache_write
                .blocks_known_by_peer
                .iter()
                .map(|(id, _)| id.clone())
                .collect();
            {
                let active_connections_read = self.active_connections.read();
                for peer_id in peers {
                    if !active_connections_read.connections.contains_key(&peer_id) {
                        cache_write.blocks_known_by_peer.pop(&peer_id);
                    }
                }
            }
        }
        {
            let active_connections_read = self.active_connections.read();
            let connection = active_connections_read
                .connections
                .get(&from_peer_id)
                .ok_or(ProtocolError::SendError(format!(
                    "Send block info peer {} isn't connected anymore",
                    &from_peer_id
                )))?;
            connection
                .send_channels
                .send(
                    &self.block_message_serializer,
                    BlockMessage::ReplyForBlocks(all_blocks_info).into(),
                    true,
                )
                .map_err(|err| {
                    ProtocolError::SendError(format!("Send block info error: {:?}", err))
                })
        }
    }

    fn on_block_info_received(
        &mut self,
        from_peer_id: PeerId,
        block_id: BlockId,
        info: BlockInfoReply,
    ) -> Result<(), ProtocolError> {
        match info {
            BlockInfoReply::Header(header) => {
                // Verify and Send it consensus
                self.on_block_header_received(from_peer_id, block_id, header)
            }
            BlockInfoReply::Info(operation_list) => {
                // Ask for missing operations ids and print a warning if there is no header for
                // that block.
                // Ban the node if the operation ids hash doesn't match with the hash contained in
                // the block_header.
                self.on_block_operation_list_received(
                    from_peer_id,
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
                {
                    let mut cache_write = self.cache.write();
                    let known_blocks =
                        cache_write
                            .blocks_known_by_peer
                            .get_or_insert_mut(from_peer_id, || {
                                LruCache::new(
                                    NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                        .expect("max_node_known_blocks_size in config must be > 0"),
                                )
                            });
                    known_blocks.put(block_id, ());
                }
                Ok(())
            }
        }
    }

    /// On block header received from a node.
    /// If the header is new, we propagate it to the consensus.
    /// We pass the state of `block_wishlist` to ask for information about the block.
    fn on_block_header_received(
        &mut self,
        from_peer_id: PeerId,
        block_id: BlockId,
        header: SecuredHeader,
    ) -> Result<(), ProtocolError> {
        if let Some(info) = self.block_wishlist.get(&block_id) {
            if info.header.is_some() {
                warn!(
                    "Peer {} sent us header for block id {} but we already received it.",
                    from_peer_id, block_id
                );
                if let Some(asked_blocks) = self.asked_blocks.get_mut(&from_peer_id) {
                    if asked_blocks.contains_key(&block_id) {
                        asked_blocks.remove(&block_id);
                        {
                            let mut cache_write = self.cache.write();
                            let blocks = cache_write.blocks_known_by_peer.get_or_insert_mut(
                                from_peer_id,
                                || {
                                    LruCache::new(
                                        NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                            .expect(
                                                "max_node_known_blocks_size in config must be > 0",
                                            ),
                                    )
                                },
                            );
                            blocks.put(block_id, ());
                        }
                    }
                }

                return Ok(());
            }
        }
        if let Err(err) = self.note_header_from_peer(&header, &from_peer_id) {
            warn!(
                "peer {} sent us critically incorrect header through protocol, \
                which may be an attack attempt by the remote node \
                or a loss of sync between us and the remote node. Err = {}",
                from_peer_id, err
            );
            let _ = self.ban_node(&from_peer_id);
            return Ok(());
        };
        if let Some(info) = self.block_wishlist.get_mut(&block_id) {
            info.header = Some(header);
        }

        // Update ask block
        // Maybe this code is useless as it's been done just above but in a condition that should cover all cases where it's useful
        // to do this. But maybe it's still trigger there it need verifications.
        let mut set = PreHashSet::<BlockId>::with_capacity(1);
        set.insert(block_id);
        self.remove_asked_blocks_of_node(&set)?;
        Ok(())
    }

    /// Perform checks on a header,
    /// and if valid update the node's view of the world.
    ///
    /// Returns a boolean representing whether the header is new.
    ///
    /// Does not ban the source node if the header is invalid.
    ///
    /// Checks performed on Header:
    /// - Not genesis.
    /// - Can compute a `BlockId`.
    /// - Valid signature.
    /// - Absence of duplicate endorsements.
    ///
    /// Checks performed on endorsements:
    /// - Unique indices.
    /// - Slot matches that of the block.
    /// - Block matches that of the block.
    pub(crate) fn note_header_from_peer(
        &mut self,
        header: &SecuredHeader,
        from_peer_id: &PeerId,
    ) -> Result<Option<(BlockId, bool)>, ProtocolError> {
        // refuse genesis blocks
        if header.content.slot.period == 0 || header.content.parents.is_empty() {
            return Ok(None);
        }

        // compute ID
        let block_id = header.id;

        // check if this header was already verified
        let now = Instant::now();
        {
            let cache_write = self.cache.write();
            if let Some(block_header) = cache_write.checked_headers.get(&block_id) {
                let blocks =
                    cache_write
                        .blocks_known_by_peer
                        .get_or_insert_mut(*from_peer_id, || {
                            LruCache::new(
                                NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                    .expect("max_node_known_blocks_size in config must be > 0"),
                            )
                        });
                blocks.put(block_id, ());
                for parent in &block_header.content.parents {
                    blocks.put(*parent, ());
                }
                {
                    let write_endorsement_cache = self.endorsement_cache.write();
                    let endorsement_ids = write_endorsement_cache
                        .endorsements_known_by_peer
                        .get_or_insert_mut(*from_peer_id, || {
                            LruCache::new(
                                NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                    .expect("max_node_known_blocks_size in config must be > 0"),
                            )
                        });
                    for endorsement_id in block_header.content.endorsements.iter().map(|e| e.id) {
                        endorsement_ids.put(endorsement_id, ());
                    }
                }
                return Ok(Some((block_id, false)));
            }
        }

        if let Err(err) =
            self.note_endorsements_from_peer(header.content.endorsements.clone(), from_peer_id)
        {
            warn!(
                "node {} sent us a header containing critically incorrect endorsements: {}",
                from_peer_id, err
            );
            return Ok(None);
        };

        // check header signature
        if let Err(err) = header.verify_signature() {
            massa_trace!("protocol.protocol_worker.check_header.err_signature", { "header": header, "err": format!("{}", err)});
            return Ok(None);
        };

        // check endorsement in header integrity
        let mut used_endorsement_indices: HashSet<u32> =
            HashSet::with_capacity(header.content.endorsements.len());
        for endorsement in header.content.endorsements.iter() {
            // check index reuse
            if !used_endorsement_indices.insert(endorsement.content.index) {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_index_reused", { "header": header, "endorsement": endorsement});
                return Ok(None);
            }
            // check slot
            if endorsement.content.slot != header.content.slot {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_invalid_slot", { "header": header, "endorsement": endorsement});
                return Ok(None);
            }
            // check endorsed block
            if endorsement.content.endorsed_block
                != header.content.parents[header.content.slot.thread as usize]
            {
                massa_trace!("protocol.protocol_worker.check_header.err_endorsement_invalid_endorsed_block", { "header": header, "endorsement": endorsement});
                return Ok(None);
            }
        }
        {
            let mut cache_write = self.cache.write();
            cache_write.checked_headers.put(block_id, header.clone());
            let blocks = cache_write
                .blocks_known_by_peer
                .get_or_insert_mut(*from_peer_id, || {
                    LruCache::new(
                        NonZeroUsize::new(self.config.max_node_known_blocks_size)
                            .expect("max_node_known_blocks_size in config must be > 0"),
                    )
                });
            blocks.put(block_id, ());
            for parent in header.content.parents {
                blocks.put(parent, ());
            }
            {
                let write_endorsement_cache = self.endorsement_cache.write();
                let endorsement_ids = write_endorsement_cache
                    .endorsements_known_by_peer
                    .get_or_insert_mut(*from_peer_id, || {
                        LruCache::new(
                            NonZeroUsize::new(self.config.max_node_known_blocks_size)
                                .expect("max_node_known_blocks_size in config must be > 0"),
                        )
                    });
                for endorsement_id in header.content.endorsements.iter().map(|e| e.id) {
                    endorsement_ids.put(endorsement_id, ());
                }
            }
        }
        massa_trace!("protocol.protocol_worker.note_header_from_node.ok", { "node": from_peer_id, "block_id": block_id, "header": header});
        Ok(Some((block_id, true)))
    }

    /// send a ban peer command to the peer handler
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .send(PeerManagementCmd::Ban(peer_id.clone()))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }

    /// Remove the given blocks from the local wishlist
    pub(crate) fn remove_asked_blocks_of_node(
        &mut self,
        remove_hashes: &PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.remove_asked_blocks_of_node", {
            "remove": remove_hashes
        });
        for asked_blocks in self.asked_blocks.values_mut() {
            asked_blocks.retain(|h, _| !remove_hashes.contains(h));
        }
        Ok(())
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
                if read_cache.checked_endorsements.contains(&endorsement_id) {
                    new_endorsements.insert(endorsement_id, endorsement);
                }
            }
        }

        // Batch signature verification
        // optimized signature verification
        verify_sigs_batch(
            &new_endorsements
                .iter()
                .map(|(endorsement_id, endorsement)| {
                    (
                        *endorsement_id.get_hash(),
                        endorsement.signature,
                        endorsement.content_creator_pub_key,
                    )
                })
                .collect::<Vec<_>>(),
        )?;

        {
            let mut cache_write = self.endorsement_cache.write();
            // add to verified signature cache
            for endorsement_id in endorsement_ids.iter() {
                cache_write.checked_endorsements.put(*endorsement_id, ());
            }
            // add to known endorsements for source node.
            let endorsements =
                cache_write
                    .endorsements_known_by_peer
                    .get_or_insert_mut(*from_peer_id, || {
                        LruCache::new(
                            NonZeroUsize::new(self.config.max_node_known_endorsements_size)
                                .expect("max_node_known_endorsements_size in config should be > 0"),
                        )
                    });
            for endorsement_id in endorsement_ids.iter() {
                endorsements.put(*endorsement_id, ());
            }
        }

        if !new_endorsements.is_empty() {
            let mut endorsements = self.storage.clone_without_refs();
            endorsements.store_endorsements(new_endorsements.into_values().collect());
            // Add to pool
            self.pool_controller.add_endorsements(endorsements);
        }

        Ok(())
    }
}

pub fn start_retrieval_thread(
    active_connections: SharedActiveConnections,
    receiver_network: Receiver<PeerMessageTuple>,
    receiver: Receiver<BlockHandlerRetrievalCommand>,
    internal_sender: Sender<BlockHandlerCommand>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
    storage: Storage,
) -> JoinHandle<()> {
    let block_message_serializer =
        MessagesSerializer::new().with_block_message_serializer(BlockMessageSerializer::new());
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            active_connections,
            receiver_network,
            block_message_serializer,
            receiver,
            internal_sender,
            cache,
            config,
            storage,
        };
        retrieval_thread.run();
    })
}
