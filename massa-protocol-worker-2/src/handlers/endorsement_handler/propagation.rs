use std::{num::NonZeroUsize, thread::JoinHandle};

use crossbeam::channel::Receiver;
use lru::LruCache;
use massa_models::{
    endorsement::{EndorsementId, SecureShareEndorsement},
    prehash::{PreHashMap, PreHashSet},
};
use massa_protocol_exports_2::ProtocolConfig;
use peernet::peer_id::PeerId;
use tracing::log::warn;

use crate::{messages::MessagesSerializer, wrap_network::ActiveConnectionsTrait};

use super::{
    cache::SharedEndorsementCache, commands_propagation::EndorsementHandlerPropagationCommand,
    messages::EndorsementMessageSerializer, EndorsementMessage,
};

struct PropagationThread {
    receiver: Receiver<EndorsementHandlerPropagationCommand>,
    config: ProtocolConfig,
    cache: SharedEndorsementCache,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    endorsement_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(msg) => {
                    match msg {
                        EndorsementHandlerPropagationCommand::PropagateEndorsements(
                            mut endorsements,
                        ) => {
                            // IMPORTANT: This is there to batch all "waiting to propagate endorsements" but will not work anymore if there is
                            // other variants in EndorsementHandlerPropagationCommand
                            while let Ok(msg) = self.receiver.try_recv() {
                                match msg {
                                    EndorsementHandlerPropagationCommand::PropagateEndorsements(
                                        endorsements2,
                                    ) => {
                                        endorsements.extend(endorsements2);
                                    }
                                    EndorsementHandlerPropagationCommand::Stop => {
                                        println!("Stop endorsement propagation thread");
                                        return;
                                    }
                                }
                            }
                            let endorsements_ids: PreHashSet<EndorsementId> = endorsements
                                .get_endorsement_refs()
                                .iter()
                                .copied()
                                .collect();
                            {
                                let mut cache_write = self.cache.write();
                                for endorsement_id in endorsements_ids.iter().copied() {
                                    cache_write.checked_endorsements.put(endorsement_id, ());
                                }
                                // Add peers that potentially don't exist in cache
                                let peer_connected =
                                    self.active_connections.get_peer_ids_connected();
                                for peer_id in &peer_connected {
                                    cache_write.endorsements_known_by_peer.put(
                                        peer_id.clone(),
                                        LruCache::new(
                                            NonZeroUsize::new(
                                                self.config.max_node_known_endorsements_size,
                                            )
                                            .expect(
                                                "max_node_known_endorsements_size in config is > 0",
                                            ),
                                        ),
                                    );
                                }
                                let peers: Vec<PeerId> = cache_write
                                    .endorsements_known_by_peer
                                    .iter()
                                    .map(|(id, _)| id.clone())
                                    .collect();
                                // Clean shared cache if peers do not exist anymore
                                for peer_id in peers {
                                    if !peer_connected.contains(&peer_id) {
                                        cache_write.endorsements_known_by_peer.pop(&peer_id);
                                    }
                                }
                                for (peer_id, endorsement_ids) in
                                    cache_write.endorsements_known_by_peer.iter_mut()
                                {
                                    let new_endorsements: PreHashMap<
                                        EndorsementId,
                                        SecureShareEndorsement,
                                    > = {
                                        let endorsements_reader = endorsements.read_endorsements();
                                        endorsements
                                            .get_endorsement_refs()
                                            .iter()
                                            .filter_map(|id| {
                                                if endorsement_ids.contains(id) {
                                                    return None;
                                                }
                                                Some((
                                                    *id,
                                                    endorsements_reader.get(id).cloned().unwrap(),
                                                ))
                                            })
                                            .collect()
                                    };
                                    for endorsement_id in new_endorsements.keys().copied() {
                                        endorsement_ids.put(endorsement_id, ());
                                    }
                                    let to_send =
                                        new_endorsements.into_values().collect::<Vec<_>>();
                                    if !to_send.is_empty() {
                                        if let Err(err) = self.active_connections.send_to_peer(
                                            peer_id,
                                            &self.endorsement_serializer,
                                            EndorsementMessage::Endorsements(to_send).into(),
                                            false,
                                        ) {
                                            warn!(
                                                "could not send endorsements batch to node {}: {}",
                                                peer_id, err
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        EndorsementHandlerPropagationCommand::Stop => {
                            println!("Stop endorsement propagation thread");
                            return;
                        }
                    }
                }
                Err(_) => {
                    println!("Stop endorsement propagation thread");
                    return;
                }
            }
        }
    }
}

pub fn start_propagation_thread(
    receiver: Receiver<EndorsementHandlerPropagationCommand>,
    cache: SharedEndorsementCache,
    config: ProtocolConfig,
    active_connections: Box<dyn ActiveConnectionsTrait>,
) -> JoinHandle<()> {
    //TODO: Here and everywhere add id to threads
    std::thread::spawn(move || {
        let endorsement_serializer = MessagesSerializer::new()
            .with_endorsement_message_serializer(EndorsementMessageSerializer::new());
        let mut propagation_thread = PropagationThread {
            receiver,
            config,
            active_connections,
            cache,
            endorsement_serializer,
        };
        propagation_thread.run();
    })
}
