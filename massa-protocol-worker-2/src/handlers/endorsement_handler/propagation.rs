use std::{collections::HashMap, thread::JoinHandle};

use crossbeam::channel::Receiver;
use massa_models::{endorsement::EndorsementId, prehash::PreHashSet};
use massa_serialization::Serializer;
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use crate::handlers::endorsement_handler::messages::EndorsementMessage;

use super::{internal_messages::InternalMessage, messages::EndorsementMessageSerializer};

struct PropagationThread {
    //TODO: Add pruning
    cache_by_peer: HashMap<PeerId, PreHashSet<EndorsementId>>,
}

pub fn start_propagation_thread(
    internal_receiver: Receiver<InternalMessage>,
    active_connections: SharedActiveConnections,
) -> JoinHandle<()> {
    //TODO: Here and everywhere add id to threads
    std::thread::spawn(move || {
        let mut propagation_thread = PropagationThread {
            cache_by_peer: HashMap::new(),
        };
        let endorsement_message_serializer = EndorsementMessageSerializer::new();
        loop {
            match internal_receiver.recv() {
                Ok(internal_message) => {
                    match internal_message {
                        InternalMessage::PropagateEndorsements((from_peer_id, endorsements)) => {
                            // Add endorsements received as known by the sender peer
                            let cached_endorsements = propagation_thread
                                .cache_by_peer
                                .entry(from_peer_id)
                                .or_insert(PreHashSet::default());
                            cached_endorsements
                                .extend(endorsements.iter().map(|endorsement| endorsement.id));

                            // Send the endorsements to all connected peers
                            let active_connections = active_connections.read();
                            for (peer_id, connection) in active_connections.connections.iter() {
                                // Filter endorsements already known by the peer
                                let mut endorsements = endorsements.clone();
                                if let Some(cached_endorsements) =
                                    propagation_thread.cache_by_peer.get_mut(peer_id)
                                {
                                    endorsements.retain(|endorsement| {
                                        if cached_endorsements.contains(&endorsement.id) {
                                            false
                                        } else {
                                            cached_endorsements.insert(endorsement.id);
                                            true
                                        }
                                    });
                                }

                                // Send the endorsements
                                let message = EndorsementMessage::Endorsements(endorsements);
                                println!("Sending message to {:?}", peer_id);
                                let mut buf = Vec::new();
                                endorsement_message_serializer
                                    .serialize(&message, &mut buf)
                                    .unwrap();
                                // TODO: Have a way to have this defined level module (const value for example)
                                // TODO: Error management
                                connection.send_channels.send(2, buf, false).unwrap();
                            }
                        }
                    }
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    return;
                }
            }
        }
    })
}
