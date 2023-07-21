use super::{
    cache::SharedEndorsementCache, commands_propagation::EndorsementHandlerPropagationCommand,
    messages::EndorsementMessageSerializer, EndorsementMessage,
};
use crate::{messages::MessagesSerializer, wrap_network::ActiveConnectionsTrait};
use massa_channel::receiver::MassaReceiver;
use massa_metrics::MassaMetrics;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;
use std::thread::JoinHandle;
use tracing::{info, log::warn};

/// Endorsements need to propagate fast, so no buffering
struct PropagationThread {
    receiver: MassaReceiver<EndorsementHandlerPropagationCommand>,
    config: ProtocolConfig,
    cache: SharedEndorsementCache,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    endorsement_serializer: MessagesSerializer,
    _metrics: MassaMetrics,
}

impl PropagationThread {
    fn run(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(EndorsementHandlerPropagationCommand::PropagateEndorsements(endorsements)) => {
                    self.propagate_endorsements(endorsements);
                }
                Ok(EndorsementHandlerPropagationCommand::Stop) => {
                    info!("Stop endorsement propagation thread");
                    return;
                }
                Err(_) => {
                    info!("Stop endorsement propagation thread");
                    return;
                }
            }
        }
    }

    /// Perform propagation of endorsements to the connected peers
    fn propagate_endorsements(&mut self, endorsements: Storage) {
        // get all the endorsements to send
        let endorsements: Vec<_> = {
            let storage_lock = endorsements.read_endorsements();
            endorsements
                .get_endorsement_refs()
                .iter()
                .filter_map(|id| storage_lock.get(id).cloned())
                .collect()
        };

        // get connected peers
        let peers_connected = self.active_connections.get_peer_ids_connected();

        // get a write lock on the cache
        let mut cache_write = self.cache.write();

        // mark that we have checked those endorsements
        for endorsement in &endorsements {
            cache_write.checked_endorsements.insert(endorsement.id, ());
        }

        // Add peers that potentially don't exist in cache and remove the ones that disconnected
        cache_write.update_cache(&peers_connected);

        // Propagate to peers
        'peer_loop: for peer_id in peers_connected {
            // write access to the cache of which endorsements are known by the peer
            let peer_knowledge = cache_write
                .endorsements_known_by_peer
                .get_mut(&peer_id)
                .expect("update_cache should have added connected peer to cache");

            // get endorsements that are not known by the peer
            let to_send: Vec<_> = endorsements
                .iter()
                .filter(|endorsement| peer_knowledge.peek(&endorsement.id).is_none())
                .collect();
            if to_send.is_empty() {
                // nothing to send to that peer
                continue;
            }

            // send by chunks
            for chunk in to_send.chunks(self.config.max_endorsements_per_message as usize) {
                if let Err(err) = self.active_connections.send_to_peer(
                    &peer_id,
                    &self.endorsement_serializer,
                    EndorsementMessage::Endorsements(chunk.iter().map(|&e| e.clone()).collect())
                        .into(),
                    false,
                ) {
                    warn!(
                        "could not send endorsements batch to node {}: {}",
                        peer_id, err
                    );
                    // try with next peer, this one is probably congested
                    continue 'peer_loop;
                }
                // sent successfully: mark peer as knowing the endorsements that were sent to it
                for endorsement in chunk {
                    peer_knowledge.insert(endorsement.id, ());
                }
            }
        }
    }
}

pub fn start_propagation_thread(
    receiver: MassaReceiver<EndorsementHandlerPropagationCommand>,
    cache: SharedEndorsementCache,
    config: ProtocolConfig,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    metrics: MassaMetrics,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("protocol-endorsement-handler-propagation".to_string())
        .spawn(move || {
            let endorsement_serializer = MessagesSerializer::new()
                .with_endorsement_message_serializer(EndorsementMessageSerializer::new());
            let mut propagation_thread = PropagationThread {
                receiver,
                config,
                active_connections,
                cache,
                endorsement_serializer,
                _metrics: metrics,
            };
            propagation_thread.run();
        })
        .expect("OS failed to start endorsement propagation thread")
}
