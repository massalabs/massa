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
use massa_models::{
    node::NodeId,
    operation::{OperationIds, Operations},
    signed::Signable,
};
use massa_network_exports::NetworkError;
use massa_protocol_exports::{ProtocolError, ProtocolPoolEvent};
use massa_time::TimeError;
use tokio::time::{sleep_until, Instant, Sleep};
use tracing::{debug, warn};

/// Structure containing a Batch of `operation_ids` we would like to ask
/// to a `node_id` now or later. Mainly used in protocol and translated into
/// simple combination of a `node_id` and `operations_ids`
pub struct OperationBatchItem {
    /// last updated at instant
    pub instant: Instant,
    /// node id
    pub node_id: NodeId,
    /// operation ids
    pub operations_ids: OperationIds,
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
        operations_ids: OperationIds,
        node_id: NodeId,
    ) -> Result<(), ProtocolError> {
        // Add to the buffer, dropping the oldest one if no capacity remains.
        if self.op_batch_buffer.len() >= self.protocol_settings.operation_batch_buffer_capacity {
            self.op_batch_buffer.pop_front();
        }
        self.op_batch_buffer.push_back(OperationBatchItem {
            instant: Instant::now(),
            node_id,
            operations_ids,
        });
        Ok(())
    }

    /// On full operations are received from the network,
    /// - Update the cache `received_operations` ids and each
    ///   `node_info.known_operations`
    /// - Notify the operations to he local node, to be propagated
    pub(crate) async fn on_operations_received(&mut self, node_id: NodeId, operations: Operations) {
        let operation_ids: OperationIds = operations
            .iter()
            .filter_map(|signed_op| match signed_op.content.compute_id() {
                Ok(op_id) => Some(op_id),
                _ => None,
            })
            .collect();
        if let Some(node_info) = self.active_nodes.get_mut(&node_id) {
            node_info.known_operations.extend(operation_ids.iter());
        }
        if self
            .note_operations_from_node(operations, &node_id, true)
            .await
            .is_err()
        {
            warn!("node {} sent us critically incorrect operation, which may be an attack attempt by the remote node or a loss of sync between us and the remote node", node_id,);
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
            .checked_add(
                self.protocol_settings
                    .asked_operations_pruning_period
                    .into(),
            )
            .ok_or(TimeError::TimeOverflowError)?;
        ask_operations_timer.set(sleep_until(instant));
        Ok(())
    }

    pub(crate) async fn update_ask_operation(
        &mut self,
        operation_batch_proc_period_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        let now = Instant::now();

        // Reset timer to the next tick.
        let next_tick = now
            .checked_add(self.protocol_settings.operation_batch_proc_period.into())
            .ok_or(TimeError::TimeOverflowError)?;
        operation_batch_proc_period_timer.set(sleep_until(next_tick));

        while let Some(batch_item) = self.op_batch_buffer.front() {
            if now < batch_item.instant {
                break;
            }

            let mut ask_set = OperationIds::default();
            for op_id in batch_item.operations_ids.iter() {
                if self.checked_operations.contains(op_id) {
                    continue;
                }
                let wish = match self.asked_operations.get_mut(op_id) {
                    Some(wish) => {
                        if wish.1.contains(&batch_item.node_id) {
                            continue; // already asked to the `node_id`
                        } else {
                            Some(wish) // already asked but at someone else
                        }
                    }
                    None => None,
                };
                if let Some(wish) = wish {
                    // Ask now if latest ask instant < now - operation_batch_proc_period.
                    if wish.0
                        < now
                            .checked_sub(self.protocol_settings.operation_batch_proc_period.into())
                            .ok_or(TimeError::TimeOverflowError)?
                    {
                        debug!(
                            "re-ask operation {:?} asked for the first time {:?} millis ago.",
                            op_id,
                            wish.0.elapsed().as_millis()
                        );
                        ask_set.insert(*op_id);
                        wish.0 = now;
                        wish.1.push(batch_item.node_id);
                    }
                } else {
                    ask_set.insert(*op_id);
                    self.asked_operations
                        .insert(*op_id, (now, vec![batch_item.node_id]));
                }
            } // EndOf for op_id in op_batch:

            if !ask_set.is_empty() {
                self.network_command_sender
                    .send_ask_for_operations(batch_item.node_id, ask_set)?;

                // Remove the item from the buffer if sending was successful.
                let _ = self.op_batch_buffer.pop_front().unwrap();
            }
        }

        Ok(())
    }

    /// Process the reception of a batch of asked operations, that means that
    /// we sent already a batch of ids in the network notifying that we already
    /// have those operations. Ask pool for the operations (will change with
    /// the shared storage)
    ///
    /// See also `on_operation_results_from_pool`
    pub(crate) async fn on_asked_operations_received(
        &mut self,
        node_id: NodeId,
        op_ids: OperationIds,
    ) -> Result<(), ProtocolError> {
        if let Some(node_info) = self.active_nodes.get_mut(&node_id) {
            for op_ids in op_ids.iter() {
                node_info.known_operations.remove(op_ids);
            }
        }
        let mut operation_ids = OperationIds::default();
        for op_id in op_ids.iter() {
            if self.checked_operations.get(op_id).is_some() {
                operation_ids.insert(*op_id);
            }
        }
        self.send_protocol_pool_event(ProtocolPoolEvent::GetOperations((node_id, operation_ids)))
            .await;
        Ok(())
    }

    /// Pool send us the operations we previously asked for
    /// Function called on
    pub(crate) async fn on_operation_results_from_pool(
        &mut self,
        node_id: NodeId,
        operations: Operations,
    ) -> Result<(), NetworkError> {
        self.network_command_sender
            .send_operations(node_id, operations)
            .await
    }
}
