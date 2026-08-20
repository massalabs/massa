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
    /// Time at which the next announcement should happen if the batch is not full before that.
    fn next_batch_deadline(&self) -> std::time::Instant {
        std::time::Instant::now()
            .checked_add(self.config.operation_announcement_interval.to_duration())
            .expect("Can't init interval op propagation")
    }

    fn run(&mut self) {
        let mut batch_deadline = self.next_batch_deadline();
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
                                    batch_deadline = self.next_batch_deadline();
                                }
                            }

                            // `recv_deadline` returns immediately whenever a message is ready,
                            // so a continuously non-empty channel would never let the timeout
                            // branch below fire. Enforce the deadline here as well to make sure
                            // announcements keep happening under a constant flow of commands.
                            if std::time::Instant::now() >= batch_deadline {
                                self.announce_ops();
                                batch_deadline = self.next_batch_deadline();
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
                    batch_deadline = self.next_batch_deadline();
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
    use massa_hash::Hash;
    use massa_models::secure_share::Id;
    use std::time::{Duration, Instant};

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
}
