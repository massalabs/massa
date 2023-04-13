use std::{
    collections::{HashSet, VecDeque, BTreeSet},
    thread::JoinHandle,
    time::Instant,
};

use crossbeam::channel::{Receiver, RecvTimeoutError, Sender};
use massa_logging::massa_trace;
use massa_models::{
    operation::{OperationId, OperationPrefixId, OperationPrefixIds, SecureShareOperation},
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    secure_share::Id,
    slot::Slot,
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use massa_time::{MassaTime, TimeError};
use peernet::peer_id::PeerId;

use crate::sig_verifier::verify_sigs_batch;
use tracing::warn;

use super::{
    cache::SharedOperationCache,
    commands_propagation::OperationHandlerCommand,
    messages::{OperationMessage, OperationMessageDeserializer, OperationMessageDeserializerArgs},
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

enum RetrievalTimer {
    NextAsk(Instant),
    NextPrune(Instant),
}

impl RetrievalTimer {
    fn get_instant(&self) -> Instant {
        match self {
            RetrievalTimer::NextAsk(instant) => *instant,
            RetrievalTimer::NextPrune(instant) => *instant,
        }
    }
}

impl Eq for RetrievalTimer {}

impl PartialEq for RetrievalTimer {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl PartialOrd for RetrievalTimer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RetrievalTimer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (RetrievalTimer::NextAsk(_), RetrievalTimer::NextAsk(_)) => std::cmp::Ordering::Equal,
            (RetrievalTimer::NextPrune(_), RetrievalTimer::NextPrune(_)) => {
                std::cmp::Ordering::Equal
            }
            (a, b) => a.get_instant().cmp(&b.get_instant()),
        }
    }
}

pub struct RetrievalThread {
    receiver: Receiver<(PeerId, u64, Vec<u8>)>,
    pool_controller: Box<dyn PoolController>,
    cache: SharedOperationCache,
    asked_operations: PreHashMap<OperationPrefixId, (Instant, Vec<PeerId>)>,
    op_batch_buffer: VecDeque<OperationBatchItem>,
    timers: BTreeSet<RetrievalTimer>,
    storage: Storage,
    config: ProtocolConfig,
    internal_sender: Sender<OperationHandlerCommand>,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Real values
        let mut operation_message_deserializer =
            OperationMessageDeserializer::new(OperationMessageDeserializerArgs {
                //TODO: Real value from config
                max_operations_prefix_ids: u32::MAX,
                max_operations: u32::MAX,
                max_datastore_value_length: u64::MAX,
                max_function_name_length: u16::MAX,
                max_parameters_size: u32::MAX,
                max_op_datastore_entry_count: u64::MAX,
                max_op_datastore_key_length: u8::MAX,
                max_op_datastore_value_length: u64::MAX,
            });
        self.timers.insert(RetrievalTimer::NextAsk(Instant::now()
        .checked_add(self.config.operation_batch_proc_period.to_duration())
        .expect("Can't add duration operation_batch_proc_period")));
        self.timers.insert(RetrievalTimer::NextPrune(Instant::now().checked_add(
            self.config.asked_operations_pruning_period.to_duration(),
        ).expect("Can't add duration operation_retrieval_prune_period")));
        loop {
            let timer = self.timers.first().expect("No timers left in operation retrieval");
            //If there is message in the channel it will receive them even if the deadline is in the past
            match self.receiver.recv_deadline(timer.get_instant()) {
                Ok((peer_id, message_id, message)) => {
                    operation_message_deserializer.set_message_id(message_id);
                    let (rest, message) = operation_message_deserializer
                        .deserialize::<DeserializeError>(&message)
                        .unwrap();
                    if !rest.is_empty() {
                        println!("Error: message not fully consumed");
                        return;
                    }
                    match message {
                        OperationMessage::Operations(ops) => {
                            if let Err(err) = self.note_operations_from_peer(ops, &peer_id) {
                                warn!("peer {} sent us critically incorrect operation, which may be an attack attempt by the remote peer or a loss of sync between us and the remote peer. Err = {}", peer_id, err);
                                //TODO: Ban
                                //let _ = self.ban_node(&peer_id).await;
                            }
                        }
                        OperationMessage::OperationsAnnouncement(announcement) => {
                            if let Err(err) =
                                self.on_operations_announcements_received(announcement, &peer_id)
                            {
                                warn!("error when processing announcement received from peer {}: Err = {}", peer_id, err);
                            }
                        }
                        OperationMessage::AskForOperations(ask) => {
                            if let Err(err) = self.on_asked_operations_received(&peer_id, ask) {
                                warn!("error when processing asked operations received from peer {}: Err = {}", peer_id, err);
                            }
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    match timer {
                        RetrievalTimer::NextAsk(_) => {
                            self.timers.pop_first();
                            if let Err(err) = self.update_ask_operation() {
                                warn!("Error in update_ask_operation: {}", err);
                            };
                            let next_ask_operations = Instant::now()
                                .checked_add(self.config.operation_batch_proc_period.to_duration())
                                .expect("Can't add duration operation_batch_proc_period");
                            self.timers
                                .insert(RetrievalTimer::NextAsk(next_ask_operations));
                        }
                        RetrievalTimer::NextPrune(_) => {
                            self.timers.pop_first();
                            self.asked_operations.clear();
                            let next_prune_operations = Instant::now()
                                .checked_add(self.config.asked_operations_pruning_period.to_duration())
                                .expect("Can't add duration operation_prune_period");
                            self.timers
                                .insert(RetrievalTimer::NextPrune(next_prune_operations));
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    println!("Disconnected");
                    return;
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
        let length = operations.len();
        let mut new_operations = PreHashMap::with_capacity(length);
        let mut received_ids = PreHashSet::with_capacity(length);
        for operation in operations {
            let operation_id = operation.id;
            if operation.serialized_size() > self.config.max_serialized_operations_size_per_block {
                return Err(ProtocolError::InvalidOperationError(format!(
                    "Operation {} exceeds max block size,  maximum authorized {} bytes but found {} bytes",
                    operation_id,
                    operation.serialized_size(),
                    self.config.max_serialized_operations_size_per_block
                )));
            };
            received_ids.insert(operation_id);

            // Check operation signature only if not already checked.
            if !self.cache.read().checked_operations.contains(&operation_id) {
                // check signature if the operation wasn't in `checked_operation`
                new_operations.insert(operation_id, operation);
            };
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
            let mut cache_write = self.cache.write();
            for op_id in new_operations.keys().copied() {
                cache_write.insert_checked_operation(op_id);
            }

            // add to known ops
            let known_ops = cache_write
                .ops_known_by_peer
                .get_or_insert_mut(source_peer_id.clone(), || HashSet::default());
            known_ops.extend(received_ids.iter().map(|id| id.prefix()));
        }

        if !new_operations.is_empty() {
            // Store operation, claim locally
            let mut ops = self.storage.clone_without_refs();
            ops.store_operations(new_operations.into_values().collect());

            // Propagate operations when their expire period isn't `max_operations_propagation_time` old.
            let mut ops_to_propagate = ops.clone();
            let operations_to_not_propagate = {
                let now = MassaTime::now()?;
                let read_operations = ops_to_propagate.read_operations();
                ops_to_propagate
                    .get_op_refs()
                    .iter()
                    .filter(|op_id| {
                        let expire_period =
                            read_operations.get(op_id).unwrap().content.expire_period;
                        let expire_period_timestamp = get_block_slot_timestamp(
                            self.config.thread_count,
                            self.config.t0,
                            self.config.genesis_timestamp,
                            Slot::new(expire_period, 0),
                        );
                        match expire_period_timestamp {
                            Ok(slot_timestamp) => {
                                slot_timestamp
                                    .saturating_add(self.config.max_operations_propagation_time)
                                    < now
                            }
                            Err(_) => true,
                        }
                    })
                    .copied()
                    .collect()
            };
            ops_to_propagate.drop_operation_refs(&operations_to_not_propagate);
            let to_announce: PreHashSet<OperationId> =
                ops_to_propagate.get_op_refs().iter().copied().collect();
            self.internal_sender
                .send(OperationHandlerCommand::AnnounceOperations(to_announce))
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
        {
            let mut cache_write = self.cache.write();
            let known_ops = cache_write
                .ops_known_by_peer
                .get_or_insert_mut(peer_id.clone(), || HashSet::default());
            known_ops.extend(op_batch.iter().copied());
        }

        // filter out the operations that we already know about
        {
            let cache_read = self.cache.read();
            op_batch.retain(|prefix| !cache_read.checked_operations_prefix.contains(prefix));
        }

        let mut ask_set = OperationPrefixIds::with_capacity(op_batch.len());
        let mut future_set = OperationPrefixIds::with_capacity(op_batch.len());
        // exactitude isn't important, we want to have a now for that function call
        let now = Instant::now();
        let mut count_reask = 0;
        for op_id in op_batch {
            let wish = match self.asked_operations.get_mut(&op_id) {
                Some(wish) => {
                    if wish.1.contains(&peer_id) {
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

        //TODO: Wait damir's answer on Discord
        // if !ask_set.is_empty() {
        //     self.network_command_sender
        //         .send_ask_for_operations(peer_id, ask_set)
        //         .await
        //         .map_err(|_| ProtocolError::ChannelError("send ask for operations failed".into()))
        // } else {
        //     Ok(())
        // }
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

    /// Process the reception of a batch of asked operations, that means that
    /// we have already sent a batch of ids in the network, notifying that we already
    /// have those operations.
    fn on_asked_operations_received(
        &mut self,
        _peer_id: &PeerId,
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
        //TODO: See with damir
        // if !ops.is_empty() {
        //     self.network_command_sender
        //         .send_operations(node_id, ops)
        //         .await?;
        // }
        Ok(())
    }
}

pub fn start_retrieval_thread(
    receiver: Receiver<(PeerId, u64, Vec<u8>)>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    config: ProtocolConfig,
    cache: SharedOperationCache,
    internal_sender: Sender<OperationHandlerCommand>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver,
            pool_controller,
            storage,
            internal_sender,
            cache,
            config,
            asked_operations: PreHashMap::default(),
            op_batch_buffer: VecDeque::new(),
            timers: BTreeSet::default()
        };
        retrieval_thread.run();
    })
}
