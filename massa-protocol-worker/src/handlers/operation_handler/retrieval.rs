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

// protocol-operation-handler-retrieval
const THREAD_NAME: &str = "poh-retrieval";
static_assertions::const_assert!(THREAD_NAME.len() < 16);

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

/// State of a peer with respect to an announced operation prefix.
#[derive(Clone, Copy)]
enum PeerAskState {
    /// an `AskForOperations` was sent to that peer at that instant
    Asked(Instant),
    /// a deferred ask for that peer is waiting in `op_batch_buffer`
    Buffered,
}

/// Ask state of an announced operation prefix.
struct AskedOperation {
    /// last instant at which that prefix was asked to any peer
    last_ask: Instant,
    /// state of the peers that announced that prefix.
    /// Kept as a `Vec` because it holds at most one entry per connected peer.
    peers: Vec<(PeerId, PeerAskState)>,
}

impl AskedOperation {
    fn peer_state(&self, peer_id: &PeerId) -> Option<PeerAskState> {
        self.peers
            .iter()
            .find(|(id, _)| id == peer_id)
            .map(|(_, state)| *state)
    }

    fn set_peer_state(&mut self, peer_id: &PeerId, state: PeerAskState) {
        match self.peers.iter_mut().find(|(id, _)| id == peer_id) {
            Some(entry) => entry.1 = state,
            None => self.peers.push((*peer_id, state)),
        }
    }

    /// Drop the `Buffered` marker of a peer, if any, so that a new announcement
    /// of that prefix by that peer is processed again.
    fn clear_buffered(&mut self, peer_id: &PeerId) {
        self.peers
            .retain(|(id, state)| id != peer_id || !matches!(state, PeerAskState::Buffered));
    }
}

pub struct RetrievalThread {
    receiver: MassaReceiver<PeerMessageTuple>,
    pool_controller: Box<dyn PoolController>,
    cache: SharedOperationCache,
    asked_operations: LruMap<OperationPrefixId, AskedOperation>,
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
                chain_id: self.config.chain_id,
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
                                // A valid message prefix followed by trailing bytes must not
                                // tear down this long-lived shared retrieval thread (doing so
                                // would let a single peer deny operation handling for everyone).
                                // A compliant peer never sends trailing bytes, so ban the
                                // sender and keep serving other peers.
                                warn!(
                                    "peer {} sent an operation message with {} unexpected trailing byte(s); banning it",
                                    peer_id,
                                    rest.len()
                                );
                                if let Err(e) = self.ban_node(&peer_id) {
                                    warn!("Error when banning node: {}", e);
                                }
                                continue;
                            }
                            match message {
                                OperationMessage::Operations(ops) => {
                                    debug!("Received operation message: Operations from {}", peer_id);
                                    if let Err(err) = note_operations_from_peer(
                                        &self.storage,
                                        &mut self.cache,
                                        &self.config,
                                        ops,
                                        &peer_id,
                                        &mut self.internal_sender,
                                        &mut self.pool_controller
                                    ) {
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

    /// On receive a batch of operation ids `op_batch` from another `peer_id`
    /// Execute the following algorithm: [redirect to GitHub](https://github.com/massalabs/massa/issues/2283#issuecomment-1040872779)
    ///
    ///```py
    ///def process_op_batch(op_batch, peer_id):
    ///    ask_set = void HashSet<OperationId>
    ///    future_set = void HashSet<OperationId>
    ///    for op_id in op_batch:
    ///        if not is_op_received(op_id):
    ///            # a deferred ask is already queued for that peer, or that peer was
    ///            # asked less than op_batch_proc_period ago: nothing to do
    ///            if peer_state(op_id, peer_id) is Buffered:
    ///                continue
    ///            if peer_state(op_id, peer_id) is Asked(t) and t >= now - op_batch_proc_period:
    ///                continue
    ///            if (op_id not in asked_ops) or (asked_ops(op_id).last_ask < now - op_batch_proc_period):
    ///                ask_set.add(op_id)
    ///                asked_ops(op_id).last_ask = now
    ///                peer_state(op_id, peer_id) = Asked(now)
    ///            else:
    ///                future_set.add(op_id)
    ///    if op_batch_buf is not full:
    ///        op_batch_buf.push(now+op_batch_proc_period, peer_id, future_set)
    ///        for op_id in future_set:
    ///            peer_state(op_id, peer_id) = Buffered
    ///    ask ask_set to peer_id
    ///```
    ///
    /// Peer states are time-bounded on purpose: an `Asked` entry only silences that
    /// peer for `op_batch_proc_period`, so a peer that disconnects before answering
    /// and reconnects gets asked again on its next announcement. The `Buffered`
    /// marker prevents repeated announcements from queueing the same deferred ask
    /// several times in `op_batch_buffer`.
    fn on_operations_announcements_received(
        &mut self,
        mut op_batch: OperationPrefixIds,
        peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
        // ignore announcement from disconnected peers
        if !self
            .active_connections
            .get_peer_ids_connected()
            .contains(peer_id)
        {
            return Ok(());
        }

        // mark sender as knowing the ops
        self.cache
            .write()
            .insert_peer_known_ops(peer_id, &op_batch.iter().copied().collect::<Vec<_>>());

        // filter out the operations that we already know about
        {
            let cache_read = self.cache.read();
            op_batch.retain(|prefix| cache_read.checked_operations_prefix.peek(prefix).is_none());
        }

        let mut ask_set = OperationPrefixIds::with_capacity(op_batch.len());
        let mut future_set = OperationPrefixIds::with_capacity(op_batch.len());
        // exactitude isn't important, we want to have a now for that function call
        let now = Instant::now();
        let proc_period = self.config.operation_batch_proc_period.to_duration();
        let mut count_reask = 0;
        for op_id in op_batch {
            let mut first_announcement = false;
            let ask_now = match self.asked_operations.get(&op_id) {
                Some(asked) => {
                    match asked.peer_state(peer_id) {
                        // a deferred ask is already queued for that peer: don't queue it twice
                        Some(PeerAskState::Buffered) => continue,
                        // that peer was asked recently: let the ask time out before asking again.
                        // The check is time-based (and not "was that peer ever asked"), so a peer
                        // that reconnects without having answered is asked again.
                        Some(PeerAskState::Asked(previous_peer_ask_time))
                            if now
                                .checked_duration_since(previous_peer_ask_time)
                                .unwrap_or_default()
                                <= proc_period =>
                        {
                            continue
                        }
                        _ => {}
                    }
                    // Ask now if latest ask instant < now - operation_batch_proc_period
                    // otherwise add in future_set
                    if now
                        .checked_duration_since(asked.last_ask)
                        .unwrap_or_default()
                        > proc_period
                    {
                        count_reask += 1;
                        asked.last_ask = now;
                        asked.set_peer_state(peer_id, PeerAskState::Asked(now));
                        true
                    } else {
                        false
                    }
                }
                None => {
                    // never announced to us before, ask immediately
                    first_announcement = true;
                    true
                }
            };
            if first_announcement {
                self.asked_operations.insert(
                    op_id,
                    AskedOperation {
                        last_ask: now,
                        peers: vec![(*peer_id, PeerAskState::Asked(now))],
                    },
                );
            }
            if ask_now {
                ask_set.insert(op_id);
            } else {
                future_set.insert(op_id);
            }
        } // EndOf for op_id in op_batch:

        if count_reask > 0 {
            massa_trace!("re-ask operations.", { "count": count_reask });
        }
        if self.op_batch_buffer.len() < self.config.operation_batch_buffer_capacity
            && !future_set.is_empty()
        {
            // Record the deferred ask so that repeated announcements of the same
            // prefixes by the same peer don't fill the buffer with duplicates.
            // Nothing is recorded when the batch is dismissed below (buffer full),
            // so the peer is free to announce those prefixes again.
            for prefix in future_set.iter() {
                if let Some(asked) = self.asked_operations.get(prefix) {
                    asked.set_peer_state(peer_id, PeerAskState::Buffered);
                }
            }
            self.op_batch_buffer.push_back(OperationBatchItem {
                instant: now
                    .checked_add(self.config.operation_batch_proc_period.into())
                    .ok_or(TimeError::TimeOverflowError)?,
                peer_id: *peer_id,
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
                    if let ProtocolError::PeerDisconnected(_) = err {
                        break;
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
            // The deferred ask is no longer buffered: drop the markers, otherwise the
            // announcement replayed just below (or any later one from that peer) would
            // be ignored forever.
            for prefix in op_batch_item.operations_prefix_ids.iter() {
                if let Some(asked) = self.asked_operations.get(prefix) {
                    asked.clear_buffered(&op_batch_item.peer_id);
                }
            }
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
                if let ProtocolError::PeerDisconnected(_) = err {
                    break;
                }
            }
        }
        Ok(())
    }

    /// send a ban peer command to the peer handler
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(vec![*peer_id]))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }
}

pub(crate) fn note_operations_from_peer(
    base_storage: &Storage,
    operations_cache: &mut SharedOperationCache,
    config: &ProtocolConfig,
    operations: Vec<SecureShareOperation>,
    source_peer_id: &PeerId,
    ops_propagation_sender: &mut MassaSender<OperationHandlerPropagationCommand>,
    pool_controller: &mut Box<dyn PoolController>,
) -> Result<(), ProtocolError> {
    massa_trace!("protocol.protocol_worker.note_operations_from_peer", { "peer": source_peer_id, "operations": operations });
    let now = MassaTime::now();

    let mut new_operations = PreHashMap::with_capacity(operations.len());
    for operation in operations {
        // ignore if op is too old
        let expire_period_timestamp = get_block_slot_timestamp(
            config.thread_count,
            config.t0,
            config.genesis_timestamp,
            Slot::new(
                operation.content.expire_period,
                operation
                    .content_creator_address
                    .get_thread(config.thread_count),
            ),
        );
        match expire_period_timestamp {
            Ok(slot_timestamp) => {
                if slot_timestamp.saturating_add(config.max_operations_propagation_time) < now {
                    continue;
                }
            }
            Err(_) => continue,
        }

        // quit if op is too big
        if operation.serialized_size() > config.max_serialized_operations_size_per_block {
            return Err(ProtocolError::InvalidOperationError(format!(
                "Operation {} exceeds max block size,  maximum authorized {} bytes but found {} bytes",
                operation.id,
                operation.serialized_size(),
                config.max_serialized_operations_size_per_block
            )));
        };

        // add to new operations
        new_operations.insert(operation.id, operation);
    }

    // all valid received ids (not only new ones) for knowledge marking
    let all_received_ids: PreHashSet<_> = new_operations.keys().copied().collect();

    // retain only new ops that are not already known
    {
        let cache_read = operations_cache.read();
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
        // mark the sender as knowing the ops it sent us:
        // this holds regardless of whether we end up retaining them locally
        let mut cache_write = operations_cache.write();
        cache_write.insert_peer_known_ops(
            source_peer_id,
            &all_received_ids
                .into_iter()
                .map(|id| id.into_prefix())
                .collect::<Vec<_>>(),
        );
    }

    if !new_operations.is_empty() {
        let new_op_ids: Vec<_> = new_operations.keys().copied().collect();

        // Store new operations, claim locally
        let mut ops = base_storage.clone_without_refs();
        ops.store_operations(new_operations.into_values().collect());

        // propagate new operations: on success the propagation thread owns a clone of the storage
        let propagated = match ops_propagation_sender.try_send(
            OperationHandlerPropagationCommand::PropagateOperations(ops.clone()),
        ) {
            Ok(()) => true,
            Err(_err) => {
                warn!("Error sending operations to propagation channel");
                false
            }
        };

        // Add to pool: on success the pool worker owns the storage
        let pooled = match pool_controller.add_operations(ops) {
            Ok(()) => true,
            Err(err) => {
                warn!("Error adding operations to pool: {}", err);
                false
            }
        };

        // Mark the operations as checked only once at least one local component
        // has taken ownership of them. Otherwise the temporary storage is dropped here
        // and marking them checked would blackhole them: we would ignore any later
        // announcement or re-delivery of operations we no longer hold.
        if propagated || pooled {
            let mut cache_write = operations_cache.write();
            for op_id in new_op_ids {
                cache_write.insert_checked_operation(op_id);
            }
        }
    }

    Ok(())
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
        .name(THREAD_NAME.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap_network::MockActiveConnectionsTraitWrapper;
    use massa_channel::MassaChannel;
    use massa_models::operation::OperationId;
    use massa_pool_exports::MockPoolControllerWrapper;
    use massa_signature::KeyPair;
    use parking_lot::RwLock;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    use super::super::cache::OperationCache;

    fn new_peer_id() -> PeerId {
        PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key())
    }

    fn new_prefix(seed: u8) -> OperationPrefixId {
        OperationId::new(massa_hash::Hash::compute_from(&[seed])).into_prefix()
    }

    /// Build a retrieval thread whose peers are all connected, recording every
    /// `AskForOperations` sent in the returned vector.
    #[allow(clippy::type_complexity)]
    fn new_retrieval_thread(
        config: ProtocolConfig,
        connected: HashSet<PeerId>,
    ) -> (RetrievalThread, Arc<RwLock<Vec<(PeerId, usize)>>>) {
        let asks: Arc<RwLock<Vec<(PeerId, usize)>>> = Arc::new(RwLock::new(Vec::new()));
        let asks_clone = asks.clone();
        let mut active_connections = MockActiveConnectionsTraitWrapper::new();
        active_connections.set_expectations(|active_connections| {
            active_connections
                .expect_get_peer_ids_connected()
                .returning(move || connected.clone());
            active_connections.expect_send_to_peer().returning(
                move |peer_id, _serializer, message, _high_priority| {
                    if let crate::messages::Message::Operation(
                        OperationMessage::AskForOperations(prefixes),
                    ) = message
                    {
                        asks_clone.write().push((*peer_id, prefixes.len()));
                    }
                    Ok(())
                },
            );
        });

        let (_sender, receiver) = MassaChannel::new("test_network".to_string(), None);
        let (_sender_ext, receiver_ext) = MassaChannel::new("test_ext".to_string(), None);
        let (internal_sender, _internal_receiver) =
            MassaChannel::new("test_internal".to_string(), None);
        let (peer_cmd_sender, _peer_cmd_receiver) =
            MassaChannel::new("test_peers".to_string(), None);

        let asked_operations = LruMap::new(ByLength::new(
            config
                .asked_operations_buffer_capacity
                .try_into()
                .expect("asked_operations_buffer_capacity in config must be > 0"),
        ));
        let thread = RetrievalThread {
            receiver,
            pool_controller: Box::new(MockPoolControllerWrapper::new()),
            cache: Arc::new(RwLock::new(OperationCache::new(1000, 1000))),
            asked_operations,
            active_connections: Box::new(active_connections),
            op_batch_buffer: VecDeque::new(),
            storage: Storage::create_root(),
            config,
            internal_sender,
            receiver_ext,
            operation_message_serializer: MessagesSerializer::new()
                .with_operation_message_serializer(OperationMessageSerializer::new()),
            peer_cmd_sender,
            _massa_metrics: MassaMetrics::new(
                false,
                "0.0.0.0:9898".parse().unwrap(),
                32,
                std::time::Duration::from_secs(5),
            )
            .0,
        };
        (thread, asks)
    }

    /// F127: while an announced prefix is on cooldown, a peer repeating the same
    /// announcement must not queue several deferred asks in `op_batch_buffer`.
    #[test]
    fn test_duplicate_announcements_during_cooldown_do_not_grow_the_batch_buffer() {
        let peer_a = new_peer_id();
        let peer_b = new_peer_id();
        let (mut thread, asks) =
            new_retrieval_thread(ProtocolConfig::default(), HashSet::from([peer_a, peer_b]));
        let prefix = new_prefix(1);
        let batch: OperationPrefixIds = [prefix].into_iter().collect();

        // peer A announces first: asked right away
        thread
            .on_operations_announcements_received(batch.clone(), &peer_a)
            .unwrap();
        assert_eq!(asks.read().len(), 1);
        assert!(thread.op_batch_buffer.is_empty());

        // peer B announces the same prefix during the cooldown: the ask is deferred once
        for _ in 0..10 {
            thread
                .on_operations_announcements_received(batch.clone(), &peer_b)
                .unwrap();
        }
        assert_eq!(
            thread.op_batch_buffer.len(),
            1,
            "repeated announcements must not queue several deferred asks"
        );
        assert_eq!(asks.read().len(), 1, "peer B must not be asked yet");

        // once the deferred batch is processed, peer B is asked and the buffer is empty
        std::thread::sleep(
            ProtocolConfig::default()
                .operation_batch_proc_period
                .to_duration(),
        );
        thread.update_ask_operation().unwrap();
        assert!(thread.op_batch_buffer.is_empty());
        assert_eq!(asks.read().last().unwrap().0, peer_b);
    }

    /// F131: a peer that was asked but never answered (typically because it
    /// disconnected in between) must be asked again when it announces the prefix
    /// after the ask cooldown, instead of being ignored forever.
    #[test]
    fn test_peer_is_asked_again_after_cooldown() {
        let peer_a = new_peer_id();
        let (mut thread, asks) =
            new_retrieval_thread(ProtocolConfig::default(), HashSet::from([peer_a]));
        let prefix = new_prefix(2);
        let batch: OperationPrefixIds = [prefix].into_iter().collect();

        thread
            .on_operations_announcements_received(batch.clone(), &peer_a)
            .unwrap();
        assert_eq!(asks.read().len(), 1);

        // same peer re-announcing during the cooldown changes nothing
        thread
            .on_operations_announcements_received(batch.clone(), &peer_a)
            .unwrap();
        assert_eq!(asks.read().len(), 1);
        assert!(thread.op_batch_buffer.is_empty());

        // after the cooldown (peer disconnected and reconnected without answering),
        // its announcement triggers a fresh ask
        std::thread::sleep(
            ProtocolConfig::default()
                .operation_batch_proc_period
                .to_duration()
                + Duration::from_millis(50),
        );
        thread
            .on_operations_announcements_received(batch, &peer_a)
            .unwrap();
        assert_eq!(asks.read().len(), 2);
        assert_eq!(asks.read().last().unwrap().0, peer_a);
    }
}
