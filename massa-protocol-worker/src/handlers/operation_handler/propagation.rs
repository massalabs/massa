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
use massa_protocol_exports::ProtocolError;
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
    // ids of the operations held in `stored_for_propagation`, maintained incrementally
    // so pruning does not need to scan the whole queue on every call. The queue batches
    // are disjoint, so this is both the set of operations referenced in `op_storage` and
    // the number of operations the cache size is capped on.
    stored_ops: PreHashSet<OperationId>,
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
                            // an operation can be propagated again while it is still
                            // queued: only queue the ones that are not already tracked, so
                            // that queue membership stays aligned with the `op_storage`
                            // refs and duplicates do not consume the cache budget twice
                            let newly_tracked: PreHashSet<OperationId> = new_ops
                                .iter()
                                .filter(|op_id| self.stored_ops.insert(**op_id))
                                .copied()
                                .collect();
                            if !newly_tracked.is_empty() {
                                self.stored_for_propagation
                                    .push_back((std::time::Instant::now(), newly_tracked));
                            }
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
            &mut self.stored_ops,
            self.config.max_operations_propagation_time.to_duration(),
            self.config.max_ops_kept_for_propagation,
        );

        // remove from storage
        self.op_storage.drop_operation_refs(&removed);
    }

    /// Remove expired and excess operation batches from the propagation queue,
    /// keeping `stored_ops` in sync with the operations queued for propagation.
    ///
    /// `stored_ops` is maintained incrementally (updated here on removal and by the
    /// caller on insertion) so that pruning costs O(number of removed operations)
    /// instead of scanning the whole queue on every call. This prevents an attacker
    /// from making each propagation command increasingly expensive by flooding the
    /// queue with many tiny batches.
    ///
    /// Because the caller never queues an operation that is already tracked, the queue
    /// batches are disjoint: the cap is enforced on distinct operations and each removed
    /// id is released exactly once, so an operation is referenced in `op_storage` if and
    /// only if it is still held by a batch.
    ///
    /// Returns the set of operation ids that were removed from the queue.
    fn prune_stored_for_propagation(
        stored_for_propagation: &mut VecDeque<(std::time::Instant, PreHashSet<OperationId>)>,
        stored_ops: &mut PreHashSet<OperationId>,
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
                for op_id in op_ids {
                    stored_ops.remove(&op_id);
                    removed.insert(op_id);
                }
            } else {
                break;
            }
        }

        // Cap cache size
        // Note that we directly remove batches of operations, not individual operations
        // to favor simplicity and performance over precision.
        while stored_ops.len() > max_ops_kept_for_propagation {
            if let Some((_t, op_ids)) = stored_for_propagation.pop_front() {
                for op_id in op_ids {
                    stored_ops.remove(&op_id);
                    removed.insert(op_id);
                }
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
        {
            let mut cache_write = self.cache.write();
            let peers_connected = self.active_connections.get_peer_ids_connected();
            cache_write.update_cache(&peers_connected);

            // Propagate to peers
            let all_keys: Vec<PeerId> = cache_write.ops_known_by_peer.keys().cloned().collect();
            for peer_id in all_keys {
                let ops = cache_write.ops_known_by_peer.get_mut(&peer_id).unwrap();
                let new_ops: Vec<OperationId> = operation_ids
                    .iter()
                    .filter(|id| ops.peek(&id.prefix()).is_none())
                    .copied()
                    .collect();
                if !new_ops.is_empty() {
                    for id in &new_ops {
                        ops.insert(id.prefix(), ());
                    }
                    debug!(
                        "Send operations announcement of len {} to {}",
                        new_ops.len(),
                        peer_id
                    );
                    for sub_list in new_ops.chunks(self.config.max_operations_per_message as usize)
                    {
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

                            if let ProtocolError::PeerDisconnected(_) = err {
                                // cache of this peer is removed in next call of cache_write.update_cache
                                break;
                            }
                        }
                    }
                }
            }
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
                stored_ops: PreHashSet::with_capacity(config.max_ops_kept_for_propagation),
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
    use massa_hash::Hash;
    use massa_models::secure_share::Id;
    use std::time::{Duration, Instant};

    fn op_id(i: u64) -> OperationId {
        OperationId::new(Hash::compute_from(&i.to_be_bytes()))
    }

    /// Push a batch of operations at the given time, mirroring the accounting done by
    /// the propagation thread on `PropagateOperations`: only the operations that are not
    /// already tracked are queued, and an empty batch is not pushed at all.
    fn push_batch_at(
        queue: &mut VecDeque<(Instant, PreHashSet<OperationId>)>,
        stored_ops: &mut PreHashSet<OperationId>,
        time: Instant,
        ops: impl IntoIterator<Item = OperationId>,
    ) {
        let newly_tracked: PreHashSet<OperationId> = ops
            .into_iter()
            .filter(|op_id| stored_ops.insert(*op_id))
            .collect();
        if !newly_tracked.is_empty() {
            queue.push_back((time, newly_tracked));
        }
    }

    fn push_batch(
        queue: &mut VecDeque<(Instant, PreHashSet<OperationId>)>,
        stored_ops: &mut PreHashSet<OperationId>,
        ops: impl IntoIterator<Item = OperationId>,
    ) {
        push_batch_at(queue, stored_ops, Instant::now(), ops);
    }

    /// The ground-truth set of queued operations, computed by scanning the whole queue
    /// (the O(n) operation the incrementally maintained set is meant to replace).
    fn real_stored_ops(
        queue: &VecDeque<(Instant, PreHashSet<OperationId>)>,
    ) -> PreHashSet<OperationId> {
        queue
            .iter()
            .flat_map(|(_, ops)| ops.iter().copied())
            .collect()
    }

    /// A flood of tiny (single-op) batches must stay bounded by the op cap, and the
    /// tracked set must stay exact so pruning never falls back to scanning.
    #[test]
    fn running_count_stays_exact_and_caps_total_ops() {
        let mut queue = VecDeque::new();
        let mut stored_ops = PreHashSet::default();
        let max_ops = 100;
        let never_expires = Duration::from_secs(3600);

        for i in 0..1000u64 {
            push_batch(&mut queue, &mut stored_ops, [op_id(i)]);
            let removed = PropagationThread::prune_stored_for_propagation(
                &mut queue,
                &mut stored_ops,
                never_expires,
                max_ops,
            );

            // the tracked set must always match the ground truth
            assert_eq!(stored_ops, real_stored_ops(&queue));
            // once the cap is reached, each new 1-op batch evicts exactly one old batch
            if i >= max_ops as u64 {
                assert_eq!(removed.len(), 1);
            }
        }

        // stored ops (== queue length for distinct 1-op batches) stays bounded by the cap
        assert_eq!(stored_ops.len(), max_ops);
        assert_eq!(queue.len(), max_ops);
    }

    /// Expired batches are dropped and the tracked set is updated accordingly.
    #[test]
    fn prune_removes_expired_batches_and_updates_count() {
        let mut queue = VecDeque::new();
        let mut stored_ops = PreHashSet::default();

        // a batch inserted "in the past", already older than the propagation time
        push_batch_at(
            &mut queue,
            &mut stored_ops,
            Instant::now() - Duration::from_secs(60),
            [op_id(1), op_id(2)],
        );

        // a fresh batch that must be kept
        push_batch(&mut queue, &mut stored_ops, [op_id(3)]);

        let removed = PropagationThread::prune_stored_for_propagation(
            &mut queue,
            &mut stored_ops,
            Duration::from_secs(30),
            1_000,
        );

        assert_eq!(removed.len(), 2);
        assert!(removed.contains(&op_id(1)));
        assert!(removed.contains(&op_id(2)));
        assert_eq!(queue.len(), 1);
        assert_eq!(stored_ops, real_stored_ops(&queue));
        assert!(stored_ops.contains(&op_id(3)));
    }

    /// Propagating an operation again while it is still queued must not queue it twice:
    /// its storage ref is dropped exactly once, when its batch is pruned.
    #[test]
    fn repropagated_op_is_only_queued_once() {
        let mut queue = VecDeque::new();
        let mut stored_ops = PreHashSet::default();
        let never_expires = Duration::from_secs(3600);

        push_batch(&mut queue, &mut stored_ops, [op_id(1)]);
        for _ in 0..10 {
            push_batch(&mut queue, &mut stored_ops, [op_id(1)]);
        }

        // the repropagations added no batch and no extra tracking
        assert_eq!(queue.len(), 1);
        assert_eq!(stored_ops.len(), 1);

        // the op is released exactly once, when its only batch is pruned
        let removed = PropagationThread::prune_stored_for_propagation(
            &mut queue,
            &mut stored_ops,
            never_expires,
            0,
        );
        assert_eq!(removed, [op_id(1)].into_iter().collect::<PreHashSet<_>>());
        assert!(queue.is_empty());
        assert!(stored_ops.is_empty());
    }

    /// Batches sharing operations, pruned in a single pass, release each op exactly once
    /// and leave no stale entry behind.
    #[test]
    fn overlapping_batches_pruned_in_one_pass_release_ops_once() {
        let mut queue = VecDeque::new();
        let mut stored_ops = PreHashSet::default();
        let past = Instant::now() - Duration::from_secs(60);

        push_batch_at(&mut queue, &mut stored_ops, past, [op_id(1), op_id(2)]);
        push_batch_at(&mut queue, &mut stored_ops, past, [op_id(2), op_id(3)]);

        // op 2 was only queued by the first batch
        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue[1].1,
            [op_id(3)].into_iter().collect::<PreHashSet<_>>()
        );

        let removed = PropagationThread::prune_stored_for_propagation(
            &mut queue,
            &mut stored_ops,
            Duration::from_secs(30),
            1_000,
        );

        assert_eq!(
            removed,
            [op_id(1), op_id(2), op_id(3)]
                .into_iter()
                .collect::<PreHashSet<_>>()
        );
        assert!(queue.is_empty());
        assert!(stored_ops.is_empty());
    }

    /// Repeatedly propagating the same operation must not consume the propagation budget
    /// several times and evict unrelated operations.
    #[test]
    fn duplicate_ops_do_not_inflate_cache_pressure() {
        let mut queue = VecDeque::new();
        let mut stored_ops = PreHashSet::default();
        let max_ops = 3;
        let never_expires = Duration::from_secs(3600);

        // one honest op, then many resubmissions of the same other op
        push_batch(&mut queue, &mut stored_ops, [op_id(0)]);
        for _ in 0..100 {
            push_batch(&mut queue, &mut stored_ops, [op_id(1)]);
            let removed = PropagationThread::prune_stored_for_propagation(
                &mut queue,
                &mut stored_ops,
                never_expires,
                max_ops,
            );
            // only 2 distinct ops are retained: nothing should ever be evicted
            assert!(removed.is_empty());
            assert_eq!(stored_ops, real_stored_ops(&queue));
        }

        // the honest op is still retained despite the flood of duplicates
        assert_eq!(queue.len(), 2);
        assert_eq!(stored_ops.len(), 2);
        assert!(stored_ops.contains(&op_id(0)));
    }
}
