use std::{collections::VecDeque, thread::JoinHandle, time::Instant};

use crossbeam::{channel::tick, select};
use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_logging::massa_trace;
use massa_metrics::MassaMetrics;
use massa_models::{
    operation::{OperationPrefixId, OperationPrefixIds, SecureShareOperation},
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    secure_share::Id,
    slot::Slot,
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use massa_time::{MassaTime, TimeError};
use schnellru::{ByLength, LruMap};

use crate::{
    handlers::peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
    messages::MessagesSerializer,
    sig_verifier::verify_sigs_batch,
    wrap_network::ActiveConnectionsTrait,
};
use tracing::{debug, info, warn};

use super::{
    cache::SharedOperationCache,
    commands_propagation::OperationHandlerPropagationCommand,
    commands_retrieval::OperationHandlerRetrievalCommand,
    messages::{OperationMessage, OperationMessageDeserializer, OperationMessageDeserializerArgs},
    OperationMessageSerializer,
};

/// Structure containing a Batch of `operation_ids` we would like to ask
/// to a `peer_id` now or later. Mainly used in protocol and translated into
/// simple combination of a `peer_id` and `operations_prefix_ids`
pub struct OperationBatchItem {
    /// last updated at instant
    pub instant: Instant,
    /// node id
    pub peer_id: PeerId,
    /// operation prefix ids
    pub operations_prefix_ids: OperationPrefixIds,
}

pub struct RetrievalThread {
    receiver: MassaReceiver<PeerMessageTuple>,
    pool_controller: Box<dyn PoolController>,
    cache: SharedOperationCache,
    asked_operations: LruMap<OperationPrefixId, (Instant, Vec<PeerId>)>,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    op_batch_buffer: VecDeque<OperationBatchItem>,
    storage: Storage,
    config: ProtocolConfig,
    internal_sender: MassaSender<OperationHandlerPropagationCommand>,
    receiver_ext: MassaReceiver<OperationHandlerRetrievalCommand>,
    operation_message_serializer: MessagesSerializer,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    _massa_metrics: MassaMetrics,
}

impl RetrievalThread {
    fn run(&mut self) {
        let operation_message_deserializer =
            OperationMessageDeserializer::new(OperationMessageDeserializerArgs {
                max_operations_prefix_ids: self.config.max_operations_per_message as u32,
                max_operations: self.config.max_operations_per_message as u32,
                max_datastore_value_length: self.config.max_op_datastore_value_length,
                max_function_name_length: self.config.max_size_function_name,
                max_parameters_size: self.config.max_size_call_sc_parameter,
                max_op_datastore_entry_count: self.config.max_op_datastore_entry_count,
                max_op_datastore_key_length: self.config.max_op_datastore_key_length,
                max_op_datastore_value_length: self.config.max_op_datastore_value_length,
            });
        let tick_ask_operations = tick(self.config.operation_batch_proc_period.to_duration());

        loop {
            select! {
                recv(self.receiver) -> msg => {
                    self.receiver.update_metrics();
                    match msg {
                        Ok((peer_id, message)) => {
                            let (rest, message) = match operation_message_deserializer
                                .deserialize::<DeserializeError>(&message) {
                                    Ok((rest, message)) => (rest, message),
                                    Err(err) => {
                                        warn!("Error when deserializing message from peer {}: Err = {}", peer_id, err);
                                        continue;
                                    }
                                };
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                OperationMessage::Operations(ops) => {
                                    debug!("Received operation message: Operations from {}", peer_id);
                                    if let Err(err) = self.note_operations_from_peer(ops, &peer_id) {
                                        warn!("peer {} sent us critically incorrect operation, which may be an attack attempt by the remote peer or a loss of sync between us and the remote peer. Err = {}", peer_id, err);

                                        if let Err(e) = self.ban_node(&peer_id) {
                                            warn!("Error when banning node: {}", e);
                                        }
                                    }
                                }
                                OperationMessage::OperationsAnnouncement(announcement) => {
                                    debug!("Received operation message: OperationsAnnouncement from {}", peer_id);
                                    if let Err(err) =
                                        self.on_operations_announcements_received(announcement, &peer_id)
                                    {
                                        warn!("error when processing announcement received from peer {}: Err = {}", peer_id, err);
                                    }
                                }
                                OperationMessage::AskForOperations(ask) => {
                                    debug!("Received operation message: AskForOperations from {}", peer_id);
                                    if let Err(err) = self.on_asked_operations_received(&peer_id, ask) {
                                        warn!("error when processing asked operations received from peer {}: Err = {}", peer_id, err);
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            info!("Stop operation retrieval thread");
                            return;
                        }
                    }
                },
                recv(self.receiver_ext) -> msg => {
                    self.receiver_ext.update_metrics();
                    match msg {
                        Ok(cmd) => match cmd {
                            OperationHandlerRetrievalCommand::Stop => {
                                info!("Stop operation retrieval thread");
                                return;
                            }
                        },
                        Err(_) => {
                            info!("Stop operation retrieval thread");
                            return;
                        }
                    }
                }
                recv(tick_ask_operations) -> _ => {
                    if let Err(err) = self.update_ask_operation() {
                        warn!("Error in update_ask_operation: {}", err);
                    };
                }
            }
        }
    }

    fn note_operations_from_peer(
        &mut self,
        operations: Vec<SecureShareOperation>,
        source_peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
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
                    operation.content.get_expire_period(),
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
            let cache_read = self.cache.read();
            new_operations.retain(|op_id, _| cache_read.checked_operations.peek(op_id).is_none());
        }

        // optimized signature verification
        verify_sigs_batch(
            &new_operations
                .iter()
                .map(|(op_id, op)| (*op_id.get_hash(), op.signature, op.content_creator_pub_key))
                .collect::<Vec<_>>(),
        )?;

        'write_cache: {
            // add to checked operations
            let mut cache_write = self.cache.write();
            for op_id in new_operations.keys().copied() {
                cache_write.insert_checked_operation(op_id);
            }

            // add to known ops
            let Ok(known_ops) = cache_write
                .ops_known_by_peer
                .get_or_insert(source_peer_id.clone(), || {
                    LruMap::new(ByLength::new(
                        self.config
                            .max_node_known_ops_size
                            .try_into()
                            .expect("max_node_known_ops_size in config must be > 0"),
                    ))
                })
                .ok_or(()) else {
                    warn!("ops_known_by_peer limitation reached");
                    break 'write_cache;
                };
            for id in all_received_ids {
                known_ops.insert(id.prefix(), ());
            }
        }

        if !new_operations.is_empty() {
            // Store new operations, claim locally
            let mut ops = self.storage.clone_without_refs();
            ops.store_operations(new_operations.into_values().collect());

            self.internal_sender
                .try_send(OperationHandlerPropagationCommand::PropagateOperations(
                    ops.clone(),
                ))
                .map_err(|err| ProtocolError::SendError(err.to_string()))?;

            // Add to pool
            self.pool_controller.add_operations(ops);
        }

        Ok(())
    }

    /// On receive a batch of operation ids `op_batch` from another `peer_id`
    /// Execute the following algorithm: [redirect to GitHub](https://github.com/massalabs/massa/issues/2283#issuecomment-1040872779)
    ///
    ///```py
    ///def process_op_batch(op_batch, peer_id):
    ///    ask_set = void HashSet<OperationId>
    ///    future_set = void HashSet<OperationId>
    ///    for op_id in op_batch:
    ///        if not is_op_received(op_id):
    ///            if (op_id not in asked_ops) or (peer_id not in asked_ops(op_id)[1]):
    ///                if (op_id not in asked_ops) or (asked_ops(op_id)[0] < now - op_batch_proc_period:
    ///                    ask_set.add(op_id)
    ///                    asked_ops(op_id)[0] = now
    ///                    asked_ops(op_id)[1].add(peer_id)
    ///                else:
    ///                    future_set.add(op_id)
    ///    if op_batch_buf is not full:
    ///        op_batch_buf.push(now+op_batch_proc_period, peer_id, future_set)
    ///    ask ask_set to peer_id
    ///```
    fn on_operations_announcements_received(
        &mut self,
        mut op_batch: OperationPrefixIds,
        peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
        // mark sender as knowing the ops
        'write_cache: {
            let mut cache_write = self.cache.write();
            let Ok(known_ops) = cache_write
                .ops_known_by_peer
                .get_or_insert(peer_id.clone(), || {
                    LruMap::new(ByLength::new(
                        self.config
                            .max_node_known_ops_size
                            .try_into()
                            .expect("max_node_known_ops_size in config must be > 0"),
                    ))
                })
                .ok_or(()) else {
                    warn!("ops_known_by_peer limitation reached");
                    break 'write_cache;
                };
            for prefix in &op_batch {
                known_ops.insert(*prefix, ());
            }
        }

        // filter out the operations that we already know about
        {
            let cache_read = self.cache.read();
            op_batch.retain(|prefix| cache_read.checked_operations_prefix.peek(prefix).is_none());
        }

        let mut ask_set = OperationPrefixIds::with_capacity(op_batch.len());
        let mut future_set = OperationPrefixIds::with_capacity(op_batch.len());
        // exactitude isn't important, we want to have a now for that function call
        let now = Instant::now();
        let mut count_reask = 0;
        for op_id in op_batch {
            let wish = match self.asked_operations.get(&op_id) {
                Some(wish) => {
                    if wish.1.contains(peer_id) {
                        continue; // already asked to the `peer_id`
                    } else {
                        Some(wish) // already asked but at someone else
                    }
                }
                None => None,
            };
            if let Some(wish) = wish {
                // Ask now if latest ask instant < now - operation_batch_proc_period
                // otherwise add in future_set
                if wish.0
                    < now
                        .checked_sub(self.config.operation_batch_proc_period.into())
                        .ok_or(TimeError::TimeOverflowError)?
                {
                    count_reask += 1;
                    ask_set.insert(op_id);
                    wish.0 = now;
                    wish.1.push(peer_id.clone());
                } else {
                    future_set.insert(op_id);
                }
            } else {
                ask_set.insert(op_id);
                self.asked_operations
                    .insert(op_id, (now, vec![peer_id.clone()]));
            }
        } // EndOf for op_id in op_batch:

        if count_reask > 0 {
            massa_trace!("re-ask operations.", { "count": count_reask });
        }
        if self.op_batch_buffer.len() < self.config.operation_batch_buffer_capacity
            && !future_set.is_empty()
        {
            self.op_batch_buffer.push_back(OperationBatchItem {
                instant: now
                    .checked_add(self.config.operation_batch_proc_period.into())
                    .ok_or(TimeError::TimeOverflowError)?,
                peer_id: peer_id.clone(),
                operations_prefix_ids: future_set,
            });
        }
        if !ask_set.is_empty() {
            debug!(
                "Send ask operations of len {} to {}",
                ask_set.len(),
                peer_id
            );
            for sub_list in ask_set
                .into_iter()
                .collect::<Vec<OperationPrefixId>>()
                .chunks(self.config.max_operations_per_message as usize)
            {
                if let Err(err) = self.active_connections.send_to_peer(
                    peer_id,
                    &self.operation_message_serializer,
                    OperationMessage::AskForOperations(
                        sub_list.iter().cloned().collect::<OperationPrefixIds>(),
                    )
                    .into(),
                    false,
                ) {
                    warn!("Failed to send AskForOperations message to peer: {}", err);
                    {
                        let mut cache_write = self.cache.write();
                        cache_write.ops_known_by_peer.remove(peer_id);
                    }
                }
            }
        }
        Ok(())
    }

    fn update_ask_operation(&mut self) -> Result<(), ProtocolError> {
        let now = Instant::now();
        while !self.op_batch_buffer.is_empty()
        // This unwrap is ok because we checked that it's not empty just before.
            && now >= self.op_batch_buffer.front().unwrap().instant
        {
            let op_batch_item = self.op_batch_buffer.pop_front().unwrap();
            self.on_operations_announcements_received(
                op_batch_item.operations_prefix_ids,
                &op_batch_item.peer_id,
            )?;
        }
        Ok(())
    }

    /// Maybe move this to propagation
    /// Process the reception of a batch of asked operations, that means that
    /// we have already sent a batch of ids in the network, notifying that we already
    /// have those operations.
    fn on_asked_operations_received(
        &mut self,
        peer_id: &PeerId,
        op_pre_ids: OperationPrefixIds,
    ) -> Result<(), ProtocolError> {
        if op_pre_ids.is_empty() {
            return Ok(());
        }

        let mut ops: Vec<SecureShareOperation> = Vec::with_capacity(op_pre_ids.len());
        {
            // Scope the lock because of the async call to `send_operations` below.
            let stored_ops = self.storage.read_operations();
            for prefix in op_pre_ids {
                let opt_op = match stored_ops
                    .get_operations_by_prefix(&prefix)
                    .and_then(|ids| ids.iter().next())
                {
                    Some(id) => stored_ops.get(id),
                    None => continue,
                };
                if let Some(op) = opt_op {
                    ops.push(op.clone());
                }
            }
        }
        debug!("Send full operations of len {} to {}", ops.len(), peer_id);
        for sub_list in ops.chunks(self.config.max_operations_per_message as usize) {
            if let Err(err) = self.active_connections.send_to_peer(
                peer_id,
                &self.operation_message_serializer,
                OperationMessage::Operations(sub_list.to_vec()).into(),
                false,
            ) {
                warn!("Failed to send Operations message to peer: {}", err);
                {
                    let mut cache_write = self.cache.write();
                    cache_write.ops_known_by_peer.remove(peer_id);
                }
            }
        }
        Ok(())
    }

    /// send a ban peer command to the peer handler
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(vec![peer_id.clone()]))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start_retrieval_thread(
    receiver: MassaReceiver<PeerMessageTuple>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    config: ProtocolConfig,
    cache: SharedOperationCache,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    receiver_ext: MassaReceiver<OperationHandlerRetrievalCommand>,
    internal_sender: MassaSender<OperationHandlerPropagationCommand>,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    massa_metrics: MassaMetrics,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("protocol-operation-handler-retrieval".to_string())
        .spawn(move || {
            let mut retrieval_thread = RetrievalThread {
                receiver,
                pool_controller,
                storage,
                internal_sender,
                receiver_ext,
                cache,
                active_connections,
                asked_operations: LruMap::new(ByLength::new(
                    config
                        .asked_operations_buffer_capacity
                        .try_into()
                        .expect("asked_operations_buffer_capacity in config must be > 0"),
                )),
                config,
                operation_message_serializer: MessagesSerializer::new()
                    .with_operation_message_serializer(OperationMessageSerializer::new()),
                op_batch_buffer: VecDeque::new(),
                peer_cmd_sender,
                _massa_metrics: massa_metrics,
            };
            retrieval_thread.run();
        })
        .expect("OS failed to start operation retrieval thread")
}
