//! Contains the implementation of the life cycle of operations
//!
//! Impement the propagation algorithm written here [redirect to github]
//! (https://github.com/massalabs/massa/issues/2283#issuecomment-1040872779).
//!
//! 1) get batches of operations ids
//! 2) ask for operations
//! 3) send batches
//! 4) answer operations

use crate::protocol_worker::ProtocolWorker;
use massa_models::{
    node::NodeId,
    operation::{OperationBatchItem, OperationIds, Operations},
    prehash::BuildMap,
    signed::Signable,
};
use massa_network_exports::NetworkError;
use massa_protocol_exports::{ProtocolError, ProtocolPoolEvent};
use massa_time::TimeError;
use std::time::Duration;
use tokio::time::{sleep_until, Instant, Sleep};
use tracing::warn;

impl ProtocolWorker {
    /// Limit the number of batch an external node can send in a second. Ban
    /// it if it exceed self.settings.max_op_batch_per_sec_per_node
    ///
    /// Call that function each time we receive a batch from a `node_id`
    ///
    /// Return true if node has been considered as bannished, false otherwise.
    async fn limitation_batches_by_node(&mut self, node_id: NodeId) -> bool {
        let now = Instant::now();
        let max = self.protocol_settings.max_op_batch_per_sec_per_node;
        match self.active_nodes.get_mut(&node_id) {
            Some(node_info) => {
                let s = now - Duration::from_secs(1);
                if let Some(&oldest) = node_info.batches_sent_history.back() {
                    // if history length >= max and oldest value older than 1 second ago
                    let is_to_old = s > oldest; // one second ago is after the oldest
                    if node_info.batches_sent_history.len() == max && is_to_old {
                        if self.ban_node(&node_id).await.is_err() {
                            warn!("ban node id {} failed", node_id);
                        }
                        return true;
                    }
                }
                node_info.batches_sent_history.push_front(now);
                if node_info.batches_sent_history.len() > max {
                    node_info.batches_sent_history.pop_back();
                }
            }
            None => {
                if self.ban_node(&node_id).await.is_err() {
                    warn!("ban node id {} failed", node_id);
                }
                return true;
            }
        };
        false
    }

    /// On receive a batch of operation ids `op_batch` from another `node_id`
    /// Execute the following algorithm: [redirect to github](https://github.com/massalabs/massa/issues/2283#issuecomment-1040872779)
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
    pub(crate) async fn on_batch_operations_received(
        &mut self,
        op_batch: OperationIds,
        node_id: NodeId,
    ) -> Result<(), ProtocolError> {
        if self.limitation_batches_by_node(node_id).await {
            return Ok(()); // return ok if we just limit (and ban) the current node
        }
        let mut ask_set =
            OperationIds::with_capacity_and_hasher(op_batch.len(), BuildMap::default());
        let mut future_set =
            OperationIds::with_capacity_and_hasher(op_batch.len(), BuildMap::default());
        // exactitude isn't important, we want to have a now for that function call
        let now = Instant::now();
        for op_id in op_batch {
            if self.checked_operations.contains(&op_id) {
                continue;
            }
            let wish = match self.asked_operations.get_mut(&op_id) {
                Some(wish) => {
                    if wish.1.contains(&node_id) {
                        continue; // already asked to the `node_id`
                    } else {
                        Some(wish)
                    }
                }
                None => None,
            };
            if let Some(wish) = wish {
                if wish.0
                    > now
                        .checked_sub(Duration::from_millis(
                            self.protocol_settings.operation_batch_proc_period,
                        ))
                        .ok_or(TimeError::TimeOverflowError)?
                {
                    future_set.insert(op_id);
                } else {
                    ask_set.insert(op_id);
                    wish.0 = now;
                    wish.1.push(node_id);
                }
            } else {
                ask_set.insert(op_id);
                self.asked_operations.insert(op_id, (now, vec![node_id]));
            }
        }
        if self.op_batch_buffer.len() < self.protocol_settings.operation_batch_buffer_capacity {
            self.op_batch_buffer.push_back(OperationBatchItem {
                instant: now
                    .checked_add(Duration::from_millis(
                        self.protocol_settings.operation_batch_proc_period,
                    ))
                    .ok_or(TimeError::TimeOverflowError)?,
                node_id,
                operations_ids: future_set,
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
    /// - Uptate the cache `received_operations` ids and each
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
            .checked_add(Duration::from_millis(
                self.protocol_settings.asked_operations_pruning_period,
            ))
            .ok_or(TimeError::TimeOverflowError)?;
        ask_operations_timer.set(sleep_until(instant));
        Ok(())
    }

    pub(crate) async fn update_ask_operation(
        &mut self,
        ask_operations_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), ProtocolError> {
        let now = Instant::now();
        // init timer
        let next_tick = now
            .checked_add(self.protocol_settings.ask_block_timeout.into())
            .ok_or(TimeError::TimeOverflowError)?;
        while !self.op_batch_buffer.is_empty()
        // This unwrap is ok because we checked that it's not empty just before.
            && Instant::now() > self.op_batch_buffer.front().unwrap().instant
        {
            let op_batch_item = self.op_batch_buffer.pop_front().unwrap();
            self.on_batch_operations_received(op_batch_item.operations_ids, op_batch_item.node_id)
                .await?;
        }
        // reset timer
        ask_operations_timer.set(sleep_until(next_tick));
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

    pub(crate) async fn send_operation_ids_batch(
        &mut self,
        propagate_operations_timer: &mut std::pin::Pin<&mut Sleep>,
    ) -> Result<(), NetworkError> {
        let max = self.protocol_settings.max_operations_per_message as usize;
        if self.op_ids_to_send.len() < max {
            return Ok(());
        }
        let operation_ids = self.op_ids_to_send.drain(..max).collect::<Vec<_>>();
        self.checked_operations.extend(operation_ids.clone());
        for (node, node_info) in self.active_nodes.iter_mut() {
            let new_ops: OperationIds = operation_ids
                .iter()
                .filter(|id| !node_info.knows_op(*id))
                .copied()
                .collect();
            node_info.insert_known_ops(
                new_ops.iter().cloned().collect(),
                self.protocol_settings.max_known_ops_size,
            );
            if !new_ops.is_empty() {
                self.network_command_sender
                    .send_operations_batch(*node, new_ops)
                    .await?;
            }
        }
        let next_tick = Instant::now()
            .checked_add(self.protocol_settings.get_batch_send_period())
            .ok_or(TimeError::TimeOverflowError)?;
        propagate_operations_timer.set(sleep_until(next_tick));
        Ok(())
    }
}
