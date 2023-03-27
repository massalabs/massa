//! Contains the implementation of the life cycle of operations
//!
//! Implement the propagation algorithm written here [redirect to GitHub]
//! (https://github.com/massalabs/massa/issues/2283#issuecomment-1040872779).
//!
//! 1) get batches of operations ids
//! 2) ask for operations
//! 3) send batches
//! 4) answer operations

use std::collections::VecDeque;

use crate::protocol_worker::ProtocolWorker;
use massa_logging::massa_trace;
use massa_models::{
    node::NodeId,
    operation::{OperationPrefixIds, SecureShareOperation},
    prehash::CapacityAllocator,
};
use massa_network_exports::NetworkCommandSenderTrait;
use massa_protocol_exports::ProtocolError;
use massa_time::TimeError;
use std::pin::Pin;
use tokio::time::{sleep_until, Instant, Sleep};
use tracing::warn;

/// Structure containing a Batch of `operation_ids` we would like to ask
/// to a `node_id` now or later. Mainly used in protocol and translated into
/// simple combination of a `node_id` and `operations_prefix_ids`
pub struct OperationBatchItem {
    /// last updated at instant
    pub instant: Instant,
    /// node id
    pub node_id: NodeId,
    /// operation prefix ids
    pub operations_prefix_ids: OperationPrefixIds,
}

/// Queue containing every `[OperationsBatchItem]` we want to ask now or later.
pub type OperationBatchBuffer = VecDeque<OperationBatchItem>;

impl ProtocolWorker {
    /// On receive a batch of operation ids `op_batch` from another `node_id`
    /// Execute the following algorithm: [redirect to GitHub](https://github.com/massalabs/massa/issues/2283#issuecomment-1040872779)
    ///
    ///```py
    ///def process_op_batch(op_batch, node_id):
    ///    ask_set = void HashSet<OperationId>
    ///    future_set = void HashSet<OperationId>
    ///    for op_id in op_batch:
    ///        if not is_op_received(op_id):
    ///            if (op_id not in asked_ops) or (node_id not in asked_ops(op_id)[1]):
    ///                if (op_id not in asked_ops) or (asked_ops(op_id)[0] < now - op_batch_proc_period:
    ///                    ask_set.add(op_id)
    ///                    asked_ops(op_id)[0] = now
    ///                    asked_ops(op_id)[1].add(node_id)
    ///                else:
    ///                    future_set.add(op_id)
    ///    if op_batch_buf is not full:
    ///        op_batch_buf.push(now+op_batch_proc_period, node_id, future_set)
    ///    ask ask_set to node_id
    ///```
    pub(crate) async fn on_operations_announcements_received(
        &mut self,
        mut op_batch: OperationPrefixIds,
        node_id: NodeId,
    ) -> Result<(), ProtocolError> {
        // mark sender as knowing the ops
        if let Some(node_info) = self.active_nodes.get_mut(&node_id) {
            node_info.insert_known_ops(op_batch.iter().copied());
        }

        // filter out the operations that we already know about
        op_batch.retain(|prefix| !self.checked_operations.contains_prefix(prefix));

        let mut ask_set = OperationPrefixIds::with_capacity(op_batch.len());
        let mut future_set = OperationPrefixIds::with_capacity(op_batch.len());
        // exactitude isn't important, we want to have a now for that function call
        let now = Instant::now();
        let mut count_reask = 0;
        for op_id in op_batch {
            let wish = match self.asked_operations.get_mut(&op_id) {
                Some(wish) => {
                    if wish.1.contains(&node_id) {
                        continue; // already asked to the `node_id`
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
                    wish.1.push(node_id);
                } else {
                    future_set.insert(op_id);
                }
            } else {
                ask_set.insert(op_id);
                self.asked_operations.insert(op_id, (now, vec![node_id]));
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
                node_id,
                operations_prefix_ids: future_set,
            });
        }

        if !ask_set.is_empty() {
            self.network_command_sender
                .send_ask_for_operations(node_id, ask_set)
                .await
                .map_err(|_| ProtocolError::ChannelError("send ask for operations failed".into()))
        } else {
            Ok(())
        }
    }

    /// On full operations are received from the network,
    /// - Update the cache `received_operations` ids and each
    ///   `node_info.known_operations`
    /// - Notify the operations to he local node, to be propagated
    pub(crate) async fn on_operations_received(
        &mut self,
        node_id: NodeId,
        operations: Vec<SecureShareOperation>,
        op_timer: &mut Pin<&mut Sleep>,
    ) {
        if let Err(err) = self
            .note_operations_from_node(operations, &node_id, op_timer)
            .await
        {
            warn!("node {} sent us critically incorrect operation, which may be an attack attempt by the remote node or a loss of sync between us and the remote node. Err = {}", node_id, err);
            let _ = self.ban_node(&node_id).await;
        }
    }

    /// Clear the `asked_operations` data structure and reset
    /// `ask_operations_timer`
    pub(crate) fn prune_asked_operations(
        &mut self,
        ask_operations_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        self.asked_operations.clear();
        // reset timer
        let instant = Instant::now()
            .checked_add(self.config.asked_operations_pruning_period.into())
            .ok_or(TimeError::TimeOverflowError)?;
        ask_operations_timer.set(sleep_until(instant));
        Ok(())
    }

    pub(crate) async fn update_ask_operation(
        &mut self,
        operation_batch_proc_period_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        let now = Instant::now();
        while !self.op_batch_buffer.is_empty()
        // This unwrap is ok because we checked that it's not empty just before.
            && now >= self.op_batch_buffer.front().unwrap().instant
        {
            let op_batch_item = self.op_batch_buffer.pop_front().unwrap();
            self.on_operations_announcements_received(
                op_batch_item.operations_prefix_ids,
                op_batch_item.node_id,
            )
            .await?;
        }
        // reset timer
        if let Some(item) = self.op_batch_buffer.front() {
            operation_batch_proc_period_timer.set(sleep_until(item.instant));
        } else {
            let next_tick = now
                .checked_add(self.config.operation_batch_proc_period.into())
                .ok_or(TimeError::TimeOverflowError)?;
            operation_batch_proc_period_timer.set(sleep_until(next_tick));
        }

        Ok(())
    }

    /// Process the reception of a batch of asked operations, that means that
    /// we have already sent a batch of ids in the network, notifying that we already
    /// have those operations.
    pub(crate) async fn on_asked_operations_received(
        &mut self,
        node_id: NodeId,
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
        if !ops.is_empty() {
            self.network_command_sender
                .send_operations(node_id, ops)
                .await?;
        }
        Ok(())
    }
}
