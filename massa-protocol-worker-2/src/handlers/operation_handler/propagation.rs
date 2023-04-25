use std::{mem, num::NonZeroUsize, thread::JoinHandle};

use crossbeam::channel::{Receiver, RecvTimeoutError};
use lru::LruCache;
use massa_logging::massa_trace;
use massa_models::operation::OperationId;
use massa_protocol_exports_2::ProtocolConfig;
use peernet::peer_id::PeerId;
use tracing::log::warn;

use crate::{
    handlers::operation_handler::OperationMessage, messages::MessagesSerializer,
    wrap_network::ActiveConnectionsTrait,
};

use super::{
    cache::SharedOperationCache, commands_propagation::OperationHandlerPropagationCommand,
    OperationMessageSerializer,
};

struct PropagationThread {
    internal_receiver: Receiver<OperationHandlerPropagationCommand>,
    active_connections: Box<dyn ActiveConnectionsTrait>,
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
                    OperationHandlerPropagationCommand::AnnounceOperations(operations_ids) => {
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
                    OperationHandlerPropagationCommand::Stop => {
                        println!("Stop operation propagation thread");
                        return;
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
            let peers_connected = self.active_connections.get_peer_ids_connected();
            // Clean shared cache if peers do not exist anymore

            for peer_id in peers {
                if !peers_connected.contains(&peer_id) {
                    cache_write.ops_known_by_peer.pop(&peer_id);
                }
            }

            // Add new potential peers
            for peer_id in peers_connected {
                cache_write.ops_known_by_peer.put(
                    peer_id.clone(),
                    LruCache::new(
                        NonZeroUsize::new(self.config.max_node_known_ops_size)
                            .expect("max_node_known_endorsements_size in config is > 0"),
                    ),
                );
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
                    if let Err(err) = self.active_connections.send_to_peer(
                        peer_id,
                        &self.operation_message_serializer,
                        OperationMessage::OperationsAnnouncement(
                            new_ops.iter().map(|id| id.into_prefix()).collect(),
                        )
                        .into(),
                        false,
                    ) {
                        warn!(
                            "Failed to send OperationsAnnouncement message to peer: {}",
                            err
                        );
                    }
                }
            }
        }
    }
}

pub fn start_propagation_thread(
    internal_receiver: Receiver<OperationHandlerPropagationCommand>,
    active_connections: Box<dyn ActiveConnectionsTrait>,
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
