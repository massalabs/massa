use std::{num::NonZeroUsize, thread::JoinHandle};

use crossbeam::channel::{Receiver, Sender};
use lru::LruCache;
use massa_models::{endorsement::EndorsementId, prehash::PreHashSet};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::ProtocolConfig;
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use tracing::warn;

use crate::handlers::{
    endorsement_handler::messages::EndorsementMessage, peer_handler::models::PeerMessageTuple,
};

use super::{
    cache::SharedEndorsementCache,
    commands_propagation::EndorsementHandlerCommand,
    messages::{EndorsementMessageDeserializer, EndorsementMessageDeserializerArgs},
};

pub struct RetrievalThread {
    receiver: Receiver<PeerMessageTuple>,
    cache: SharedEndorsementCache,
    internal_sender: Sender<EndorsementHandlerCommand>,
    pool_controller: Box<dyn PoolController>,
    config: ProtocolConfig,
    storage: Storage,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Real values
        let mut endorsement_message_deserializer =
            EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
                thread_count: 32,
                max_length_endorsements: 10000,
                endorsement_count: 32,
            });
        loop {
            match self.receiver.recv() {
                Ok((peer_id, message_id, message)) => {
                    endorsement_message_deserializer.set_message_id(message_id);
                    let (rest, message) = endorsement_message_deserializer
                        .deserialize::<DeserializeError>(&message)
                        .unwrap();
                    if !rest.is_empty() {
                        println!("Error: message not fully consumed");
                        return;
                    }
                    match message {
                        EndorsementMessage::Endorsements(mut endorsements) => {
                            // Retain endorsements not in cache in endorsement vec and add them
                            {
                                let ids: PreHashSet<EndorsementId> = endorsements
                                    .iter()
                                    .map(|endorsement| endorsement.id)
                                    .collect();
                                let mut cache_write = self.cache.write();
                                // Add endorsements known for this peer
                                let cached_endorsements = cache_write
                                    .endorsements_known_by_peer
                                    .get_or_insert_mut(peer_id, || LruCache::new(NonZeroUsize::new(self.config.max_node_known_endorsements_size).expect("max_node_known_endorsements_size in config should be > 0")));
                                for id in ids {
                                    cached_endorsements.put(id, ());
                                }
                                endorsements.retain(|endorsement| {
                                    if cache_write
                                        .checked_endorsements
                                        .get(&endorsement.id)
                                        .is_some()
                                    {
                                        false
                                    } else {
                                        cache_write.insert_checked_endorsement(endorsement.id);
                                        true
                                    }
                                });
                            }
                            let mut endorsements_storage = self.storage.clone_without_refs();
                            endorsements_storage.store_endorsements(endorsements.clone());
                            self.pool_controller
                                .add_endorsements(endorsements_storage.clone());
                            if let Err(err) = self.internal_sender.send(
                                EndorsementHandlerCommand::PropagateEndorsements(
                                    endorsements_storage,
                                ),
                            ) {
                                warn!("Failed to send from retrieval thread of endorsement handler to propagation: {:?}", err);
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
    }
}

pub fn start_retrieval_thread(
    receiver: Receiver<PeerMessageTuple>,
    internal_sender: Sender<EndorsementHandlerCommand>,
    cache: SharedEndorsementCache,
    pool_controller: Box<dyn PoolController>,
    config: ProtocolConfig,
    storage: Storage,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver,
            cache,
            internal_sender,
            pool_controller,
            config,
            storage,
        };
        retrieval_thread.run();
    })
}
