use std::thread::JoinHandle;

use crossbeam::channel::Receiver;
use massa_models::{
    endorsement::{EndorsementId, SecureShareEndorsement},
    prehash::{PreHashMap, PreHashSet},
};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::ProtocolConfig;
use tracing::{debug, info, log::warn};

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
                                        info!("Stop endorsement propagation thread");
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
                                    cache_write.checked_endorsements.insert(endorsement_id, ());
                                }
                                // Add peers that potentially don't exist in cache
                                let peers_connected =
                                    self.active_connections.get_peer_ids_connected();
                                cache_write.update_cache(
                                    peers_connected,
                                    self.config
                                        .max_node_known_endorsements_size
                                        .try_into()
                                        .expect("max_node_known_endorsements_size is too big"),
                                );
                                let all_keys: Vec<PeerId> = cache_write
                                    .endorsements_known_by_peer
                                    .iter()
                                    .map(|(k, _)| k)
                                    .cloned()
                                    .collect();
                                for peer_id in all_keys.iter() {
                                    let endorsement_ids = cache_write
                                        .endorsements_known_by_peer
                                        .peek_mut(peer_id)
                                        .unwrap();
                                    let new_endorsements: PreHashMap<
                                        EndorsementId,
                                        SecureShareEndorsement,
                                    > = {
                                        let endorsements_reader = endorsements.read_endorsements();
                                        endorsements
                                            .get_endorsement_refs()
                                            .iter()
                                            .filter_map(|id| {
                                                if endorsement_ids.peek(id).is_some() {
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
                                        endorsement_ids.insert(endorsement_id, ());
                                    }
                                    let to_send =
                                        new_endorsements.into_values().collect::<Vec<_>>();
                                    if !to_send.is_empty() {
                                        debug!(
                                            "Send endorsements of len {} to {}",
                                            to_send.len(),
                                            peer_id
                                        );
                                        for sub_list in to_send.chunks(
                                            self.config.max_endorsements_per_message as usize,
                                        ) {
                                            if let Err(err) = self.active_connections.send_to_peer(
                                                peer_id,
                                                &self.endorsement_serializer,
                                                EndorsementMessage::Endorsements(sub_list.to_vec())
                                                    .into(),
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
                        }
                        EndorsementHandlerPropagationCommand::Stop => {
                            info!("Stop endorsement propagation thread");
                            return;
                        }
                    }
                }
                Err(_) => {
                    info!("Stop endorsement propagation thread");
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
            };
            propagation_thread.run();
        })
        .expect("OS failed to start endorsement propagation thread")
}
