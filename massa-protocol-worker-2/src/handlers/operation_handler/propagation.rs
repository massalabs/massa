use std::{mem, thread::JoinHandle};

use crossbeam::channel::{Receiver, RecvTimeoutError};
use massa_logging::massa_trace;
use massa_models::operation::OperationId;
use massa_protocol_exports_2::ProtocolConfig;
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};
use tracing::debug;

use crate::{handlers::operation_handler::OperationMessage, messages::MessagesSerializer};

use super::{
    cache::SharedOperationCache, commands_propagation::OperationHandlerCommand,
    OperationMessageSerializer,
};

struct PropagationThread {
    internal_receiver: Receiver<OperationHandlerCommand>,
    active_connections: SharedActiveConnections,
    operations_to_announce: Vec<OperationId>,
    config: ProtocolConfig,
    cache: SharedOperationCache,
    operation_message_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        let mut next_announce = std::time::Instant::now()
            .checked_add(self.config.operation_announcement_interval.to_duration())
            .expect("Can't init interval op propagation");
        loop {
            match self.internal_receiver.recv_deadline(next_announce) {
                Ok(internal_message) => match internal_message {
                    OperationHandlerCommand::AnnounceOperations(operations_ids) => {
                        // Note operations as checked.
                        {
                            let mut cache_write = self.cache.write();
                            for op_id in operations_ids.iter().copied() {
                                cache_write.insert_checked_operation(op_id);
                            }
                        }
                        self.operations_to_announce.extend(operations_ids);
                        if self.operations_to_announce.len()
                            > self.config.operation_announcement_buffer_capacity
                        {
                            self.announce_ops();
                            next_announce = std::time::Instant::now()
                                .checked_add(
                                    self.config.operation_announcement_interval.to_duration(),
                                )
                                .expect("Can't init interval op propagation");
                        }
                    }
                },
                Err(RecvTimeoutError::Timeout) => {
                    self.announce_ops();
                    next_announce = std::time::Instant::now()
                        .checked_add(self.config.operation_announcement_interval.to_duration())
                        .expect("Can't init interval op propagation");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return;
                }
            }
        }
    }

    fn announce_ops(&mut self) {
        // Quit if empty  to avoid iterating on nodes
        if self.operations_to_announce.is_empty() {
            return;
        }
        let operation_ids = mem::take(&mut self.operations_to_announce);
        massa_trace!("protocol.protocol_worker.announce_ops.begin", {
            "operation_ids": operation_ids
        });
        {
            let mut cache_write = self.cache.write();
            let peers: Vec<PeerId> = cache_write
                .ops_known_by_peer
                .iter()
                .map(|(id, _)| id.clone())
                .collect();
            // Clean shared cache if peers do not exist anymore
            for peer_id in peers {
                if !self
                    .active_connections
                    .read()
                    .connections
                    .contains_key(&peer_id)
                {
                    cache_write.ops_known_by_peer.pop(&peer_id);
                }
            }
            // Propagate to peers
            for (peer_id, ops) in cache_write.ops_known_by_peer.iter_mut() {
                let new_ops: Vec<OperationId> = operation_ids
                    .iter()
                    .filter(|id| !ops.contains(&id.prefix()))
                    .copied()
                    .collect();
                if !new_ops.is_empty() {
                    for id in &new_ops {
                        ops.put(id.prefix(), ());
                    }
                    {
                        let mut active_connections = self.active_connections.write();
                        if let Some(connection) = active_connections.connections.get_mut(peer_id) {
                            if let Err(err) = connection.send_channels.send(
                                &self.operation_message_serializer,
                                OperationMessage::OperationsAnnouncement(
                                    new_ops.iter().map(|id| id.into_prefix()).collect(),
                                )
                                .into(),
                                false,
                            ) {
                                debug!(
                                    "could not send operation batch to node {}: {}",
                                    peer_id, err
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn start_propagation_thread(
    internal_receiver: Receiver<OperationHandlerCommand>,
    active_connections: SharedActiveConnections,
    config: ProtocolConfig,
    cache: SharedOperationCache,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut propagation_thread = PropagationThread {
            internal_receiver,
            active_connections,
            operations_to_announce: Vec::new(),
            config,
            cache,
            operation_message_serializer: MessagesSerializer::new()
                .with_operation_message_serializer(OperationMessageSerializer::new()),
        };
        propagation_thread.run();
    })
}
