use std::collections::VecDeque;
use std::{mem, thread::JoinHandle};

use crossbeam::channel::RecvTimeoutError;
use massa_channel::receiver::MassaReceiver;
use massa_logging::massa_trace;
use massa_metrics::MassaMetrics;
use massa_models::operation::OperationId;
use massa_models::prehash::CapacityAllocator;
use massa_models::prehash::PreHashSet;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;
use tracing::{debug, info, log::warn};

use crate::{
    handlers::operation_handler::OperationMessage, messages::MessagesSerializer,
    wrap_network::ActiveConnectionsTrait,
};

use super::{
    cache::SharedOperationCache, commands_propagation::OperationHandlerPropagationCommand,
    OperationMessageSerializer,
};

// protocol-operation-handler-propagation
const THREAD_NAME: &str = "poh-tester";
static_assertions::const_assert!(THREAD_NAME.len() < 16);

struct PropagationThread {
    internal_receiver: MassaReceiver<OperationHandlerPropagationCommand>,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    // times at which previous ops were announced
    stored_for_propagation: VecDeque<(std::time::Instant, PreHashSet<OperationId>)>,
    // running total number of operations held in `stored_for_propagation`, maintained
    // incrementally so pruning does not need to scan the whole queue on every call
    stored_ops_count: usize,
    op_storage: Storage,
    next_batch: PreHashSet<OperationId>,
    config: ProtocolConfig,
    cache: SharedOperationCache,
    operation_message_serializer: MessagesSerializer,
    _massa_metrics: MassaMetrics,
}

impl PropagationThread {
    fn run(&mut self) {
        let mut batch_deadline = std::time::Instant::now()
            .checked_add(self.config.operation_announcement_interval.to_duration())
            .expect("Can't init interval op propagation");
        loop {
            match self.internal_receiver.recv_deadline(batch_deadline) {
                Ok(internal_message) => {
                    match internal_message {
                        OperationHandlerPropagationCommand::PropagateOperations(operations) => {
                            // Note operations as checked.
                            {
                                let mut cache_write = self.cache.write();
                                for op_id in operations.get_op_refs().iter().copied() {
                                    cache_write.insert_checked_operation(op_id);
                                }
                            }

                            // add to propagation storage
                            let new_ops = operations.get_op_refs().clone();
                            self.stored_ops_count =
                                self.stored_ops_count.saturating_add(new_ops.len());
                            self.stored_for_propagation
                                .push_back((std::time::Instant::now(), new_ops.clone()));
                            self.op_storage.extend(operations);
                            self.prune_propagation_storage();

                            for op_id in new_ops {
                                self.next_batch.insert(op_id);
                                if self.next_batch.len()
                                    >= self.config.operation_announcement_buffer_capacity
                                {
                                    self.announce_ops();
                                    batch_deadline = std::time::Instant::now()
                                        .checked_add(
                                            self.config
                                                .operation_announcement_interval
                                                .to_duration(),
                                        )
                                        .expect("Can't init interval op propagation");
                                }
                            }
                        }
                        OperationHandlerPropagationCommand::Stop => {
                            info!("Stop operation propagation thread");
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.announce_ops();
                    batch_deadline = std::time::Instant::now()
                        .checked_add(self.config.operation_announcement_interval.to_duration())
                        .expect("Can't init interval op propagation");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return;
                }
            }
        }
    }

    /// Prune the list of operations kept for propagation.
    fn prune_propagation_storage(&mut self) {
        let removed = Self::prune_stored_for_propagation(
            &mut self.stored_for_propagation,
            &mut self.stored_ops_count,
            self.config.max_operations_propagation_time.to_duration(),
            self.config.max_ops_kept_for_propagation,
        );

        // remove from storage
        self.op_storage.drop_operation_refs(&removed);
    }

    /// Remove expired and excess operation batches from the propagation queue,
    /// keeping `stored_ops_count` in sync with the total number of queued operations.
    ///
    /// `stored_ops_count` is maintained incrementally (updated here on removal and by
    /// the caller on insertion) so that pruning costs O(number of removed batches)
    /// instead of scanning the whole queue on every call. This prevents an attacker
    /// from making each propagation command increasingly expensive by flooding the
    /// queue with many tiny batches.
    ///
    /// Returns the set of operation ids that were removed from the queue.
    fn prune_stored_for_propagation(
        stored_for_propagation: &mut VecDeque<(std::time::Instant, PreHashSet<OperationId>)>,
        stored_ops_count: &mut usize,
        max_operations_propagation_time: std::time::Duration,
        max_ops_kept_for_propagation: usize,
    ) -> PreHashSet<OperationId> {
        let mut removed = PreHashSet::default();

        // remove expired
        while let Some((t, _)) = stored_for_propagation.front() {
            if t.elapsed() > max_operations_propagation_time {
                let (_, op_ids) = stored_for_propagation
                    .pop_front()
                    .expect("there should be at least one element, checked above");
                *stored_ops_count = stored_ops_count.saturating_sub(op_ids.len());
                removed.extend(op_ids);
            } else {
                break;
            }
        }

        // Cap cache size
        // Note that we directly remove batches of operations, not individual operations
        // to favor simplicity and performance over precision.
        while *stored_ops_count > max_ops_kept_for_propagation {
            if let Some((_t, op_ids)) = stored_for_propagation.pop_front() {
                *stored_ops_count = stored_ops_count.saturating_sub(op_ids.len());
                removed.extend(op_ids);
            } else {
                break;
            }
        }

        removed
    }

    fn announce_ops(&mut self) {
        // Quit if empty  to avoid iterating on nodes
        if self.next_batch.is_empty() {
            return;
        }
        let operation_ids = mem::take(&mut self.next_batch);
        massa_trace!("protocol.protocol_worker.announce_ops.begin", {
            "operation_ids": operation_ids
        });
        // Operations whose announcement could not be delivered to at least one peer.
        // They are put back into the next batch so that a later round retries them.
        let mut to_retry: PreHashSet<OperationId> = PreHashSet::default();
        {
            let mut cache_write = self.cache.write();
            let peers_connected = self.active_connections.get_peer_ids_connected();
            cache_write.update_cache(&peers_connected);

            // Propagate to peers
            let all_keys: Vec<PeerId> = cache_write.ops_known_by_peer.keys().cloned().collect();
            let chunk_size = self.config.max_operations_per_message as usize;
            'peer_loop: for peer_id in all_keys {
                let ops = cache_write.ops_known_by_peer.get_mut(&peer_id).unwrap();
                let new_ops: Vec<OperationId> = operation_ids
                    .iter()
                    .filter(|id| ops.peek(&id.prefix()).is_none())
                    .copied()
                    .collect();
                if !new_ops.is_empty() {
                    debug!(
                        "Send operations announcement of len {} to {}",
                        new_ops.len(),
                        peer_id
                    );
                    for (chunk_index, sub_list) in new_ops.chunks(chunk_size).enumerate() {
                        if let Err(err) = self.active_connections.send_to_peer(
                            &peer_id,
                            &self.operation_message_serializer,
                            OperationMessage::OperationsAnnouncement(
                                sub_list.iter().map(|id| id.into_prefix()).collect(),
                            )
                            .into(),
                            false,
                        ) {
                            warn!(
                                "Failed to send OperationsAnnouncement message to peer: {}",
                                err
                            );
                            // Nothing was delivered from this chunk on: the peer's cache is left
                            // untouched for those operations, and they are retried later.
                            to_retry.extend(
                                new_ops[chunk_index.saturating_mul(chunk_size)..]
                                    .iter()
                                    .copied(),
                            );
                            // this peer is disconnected or congested, try with the next one
                            // (a disconnected peer's cache is removed by the next
                            // cache_write.update_cache call)
                            continue 'peer_loop;
                        }
                        // sent successfully: only now mark the peer as knowing those operations
                        for id in sub_list {
                            ops.insert(id.prefix(), ());
                        }
                    }
                }
            }
        }

        // Requeue the operations that still need to be announced. Only the ones we still
        // hold for propagation are worth retrying, and the batch is kept bounded so that a
        // permanently congested peer cannot make it grow without limit.
        if !to_retry.is_empty() {
            let stored_ops = self.op_storage.get_op_refs();
            let retry: Vec<OperationId> = to_retry
                .into_iter()
                .filter(|op_id| stored_ops.contains(op_id))
                .take(
                    self.config
                        .operation_announcement_buffer_capacity
                        .saturating_sub(self.next_batch.len()),
                )
                .collect();
            self.next_batch.extend(retry);
        }
    }
}

pub fn start_propagation_thread(
    internal_receiver: MassaReceiver<OperationHandlerPropagationCommand>,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    config: ProtocolConfig,
    cache: SharedOperationCache,
    op_storage: Storage,
    massa_metrics: MassaMetrics,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(THREAD_NAME.to_string())
        .spawn(move || {
            let mut propagation_thread = PropagationThread {
                internal_receiver,
                active_connections,
                stored_for_propagation: VecDeque::with_capacity(
                    config.max_ops_kept_for_propagation,
                ),
                stored_ops_count: 0,
                op_storage,
                next_batch: PreHashSet::with_capacity(
                    config
                        .operation_announcement_buffer_capacity
                        .saturating_add(1),
                ),
                config,
                cache,
                _massa_metrics: massa_metrics,
                operation_message_serializer: MessagesSerializer::new()
                    .with_operation_message_serializer(OperationMessageSerializer::new()),
            };
            propagation_thread.run();
        })
        .expect("OS failed to start operation propagation thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wrap_network::MockActiveConnectionsTrait;
    use massa_channel::MassaChannel;
    use massa_hash::Hash;
    use massa_models::address::Address;
    use massa_models::amount::Amount;
    use massa_models::operation::{Operation, OperationSerializer, OperationType};
    use massa_models::secure_share::Id;
    use massa_models::secure_share::SecureShareContent;
    use massa_protocol_exports::ProtocolError;
    use massa_signature::KeyPair;
    use parking_lot::RwLock;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::super::cache::OperationCache;

    fn op_id(i: u64) -> OperationId {
        OperationId::new(Hash::compute_from(&i.to_be_bytes()))
    }

    /// Push a batch of operations, mirroring the accounting done by the propagation
    /// thread on `PropagateOperations` (increment the running count, then push).
    fn push_batch(
        queue: &mut VecDeque<(Instant, PreHashSet<OperationId>)>,
        count: &mut usize,
        ops: impl IntoIterator<Item = OperationId>,
    ) {
        let ops: PreHashSet<OperationId> = ops.into_iter().collect();
        *count = count.saturating_add(ops.len());
        queue.push_back((Instant::now(), ops));
    }

    /// The ground-truth op count, computed by scanning the whole queue (the O(n)
    /// operation the running count is meant to replace).
    fn real_count(queue: &VecDeque<(Instant, PreHashSet<OperationId>)>) -> usize {
        queue.iter().map(|(_, ops)| ops.len()).sum()
    }

    /// A flood of tiny (single-op) batches must stay bounded by the op cap, and the
    /// running count must stay exact so pruning never falls back to scanning.
    #[test]
    fn running_count_stays_exact_and_caps_total_ops() {
        let mut queue = VecDeque::new();
        let mut count = 0usize;
        let max_ops = 100;
        let never_expires = Duration::from_secs(3600);

        for i in 0..1000u64 {
            push_batch(&mut queue, &mut count, [op_id(i)]);
            let removed = PropagationThread::prune_stored_for_propagation(
                &mut queue,
                &mut count,
                never_expires,
                max_ops,
            );

            // running count must always match the ground truth
            assert_eq!(count, real_count(&queue));
            // once the cap is reached, each new 1-op batch evicts exactly one old batch
            if i >= max_ops as u64 {
                assert_eq!(removed.len(), 1);
            }
        }

        // total ops (== queue length for 1-op batches) stays bounded by the cap
        assert_eq!(count, max_ops);
        assert_eq!(queue.len(), max_ops);
        assert_eq!(count, real_count(&queue));
    }

    /// Expired batches are dropped and the running count is decremented accordingly.
    #[test]
    fn prune_removes_expired_batches_and_updates_count() {
        let mut queue = VecDeque::new();
        let mut count = 0usize;

        // a batch inserted "in the past", already older than the propagation time
        let expired: PreHashSet<OperationId> = [op_id(1), op_id(2)].into_iter().collect();
        count = count.saturating_add(expired.len());
        queue.push_back((Instant::now() - Duration::from_secs(60), expired));

        // a fresh batch that must be kept
        push_batch(&mut queue, &mut count, [op_id(3)]);

        let removed = PropagationThread::prune_stored_for_propagation(
            &mut queue,
            &mut count,
            Duration::from_secs(30),
            1_000,
        );

        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&op_id(1)));
        assert!(removed.contains(&op_id(2)));
        assert_eq!(queue.len(), 1);
        assert_eq!(count, 1);
        assert_eq!(count, real_count(&queue));
    }

    /// Build a propagation thread wired to `active_connections`, holding `ops` for
    /// propagation and with them queued in the next announcement batch.
    fn propagation_thread_with_batch(
        active_connections: MockActiveConnectionsTrait,
        ops: &[massa_models::operation::SecureShareOperation],
    ) -> PropagationThread {
        let (_sender, receiver) =
            MassaChannel::new::<OperationHandlerPropagationCommand>("test".to_string(), None);
        let (metrics, _stopper) = MassaMetrics::new(
            false,
            "0.0.0.0:31248".parse().unwrap(),
            2,
            Duration::from_secs(5),
        );
        let mut op_storage = Storage::create_root();
        op_storage.store_operations(ops.to_vec());
        PropagationThread {
            internal_receiver: receiver,
            active_connections: Box::new(active_connections),
            stored_for_propagation: VecDeque::new(),
            stored_ops_count: 0,
            op_storage,
            next_batch: ops.iter().map(|op| op.id).collect(),
            config: ProtocolConfig::default(),
            cache: Arc::new(RwLock::new(OperationCache::new(1000, 1000))),
            operation_message_serializer: MessagesSerializer::new()
                .with_operation_message_serializer(OperationMessageSerializer::new()),
            _massa_metrics: metrics,
        }
    }

    fn test_operation(index: u64) -> massa_models::operation::SecureShareOperation {
        let keypair = KeyPair::generate(0).unwrap();
        let content = Operation {
            fee: Amount::default(),
            op: OperationType::Transaction {
                recipient_address: Address::from_public_key(&keypair.get_public_key()),
                amount: Amount::default(),
            },
            expire_period: index,
        };
        Operation::new_verifiable(content, OperationSerializer::new(), &keypair, 0).unwrap()
    }

    /// A failed announcement must not mark the peer as knowing the operations, and the
    /// unsent operations must be requeued so a later round retries them. Otherwise the
    /// filtering done on the next round would suppress them forever for that peer.
    #[test]
    fn failed_announcement_is_not_cached_and_is_requeued() {
        let peer_id = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
        let ops: Vec<_> = (0..3).map(test_operation).collect();
        let op_ids: PreHashSet<OperationId> = ops.iter().map(|op| op.id).collect();

        let mut active_connections = MockActiveConnectionsTrait::new();
        active_connections
            .expect_get_peer_ids_connected()
            .returning(move || HashSet::from_iter([peer_id]));
        active_connections
            .expect_send_to_peer()
            .returning(|_, _, _, _| Err(ProtocolError::SendError("congested".to_string())));

        let mut propagation_thread = propagation_thread_with_batch(active_connections, &ops);
        propagation_thread.announce_ops();

        // the peer knows none of the operations, so they will be announced again
        let cache_read = propagation_thread.cache.read();
        let known_by_peer = cache_read.ops_known_by_peer.get(&peer_id).unwrap();
        for op_id in &op_ids {
            assert!(known_by_peer.peek(&op_id.prefix()).is_none());
        }
        drop(cache_read);

        // the operations are queued again for the next announcement round
        assert_eq!(propagation_thread.next_batch, op_ids);
    }

    /// On a successful announcement the peer is marked as knowing the operations and
    /// nothing is requeued.
    #[test]
    fn successful_announcement_is_cached_and_not_requeued() {
        let peer_id = PeerId::from_public_key(KeyPair::generate(0).unwrap().get_public_key());
        let ops: Vec<_> = (0..3).map(test_operation).collect();
        let op_ids: PreHashSet<OperationId> = ops.iter().map(|op| op.id).collect();

        let mut active_connections = MockActiveConnectionsTrait::new();
        active_connections
            .expect_get_peer_ids_connected()
            .returning(move || HashSet::from_iter([peer_id]));
        active_connections
            .expect_send_to_peer()
            .returning(|_, _, _, _| Ok(()));

        let mut propagation_thread = propagation_thread_with_batch(active_connections, &ops);
        propagation_thread.announce_ops();

        let cache_read = propagation_thread.cache.read();
        let known_by_peer = cache_read.ops_known_by_peer.get(&peer_id).unwrap();
        for op_id in &op_ids {
            assert!(known_by_peer.peek(&op_id.prefix()).is_some());
        }
        drop(cache_read);

        assert!(propagation_thread.next_batch.is_empty());
    }
}
