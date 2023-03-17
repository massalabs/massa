use std::thread::JoinHandle;

use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use massa_models::{endorsement::EndorsementId, prehash::PreHashSet};
use massa_pool_exports::PoolController;
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use peernet::peer_id::PeerId;

use crate::handlers::endorsement_handler::messages::EndorsementMessage;

use super::{
    commands::EndorsementHandlerCommand,
    internal_messages::InternalMessage,
    messages::{EndorsementMessageDeserializer, EndorsementMessageDeserializerArgs},
};

pub struct RetrievalThread {
    receiver: Receiver<(PeerId, Vec<u8>)>,
    receiver_ext: Receiver<EndorsementHandlerCommand>,
    cached_endorsement_ids: PreHashSet<EndorsementId>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    internal_sender: Sender<InternalMessage>,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Real values
        let endorsement_message_deserializer =
            EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
                thread_count: 32,
                max_length_endorsements: 10000,
                endorsement_count: 32,
            });
        loop {
            select! {
                recv(self.receiver) -> msg => {
                    match msg {
                        Ok((peer_id, message)) => {
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
                                    endorsements.retain(|endorsement| {
                                        if self.cached_endorsement_ids.contains(&endorsement.id) {
                                            false
                                        } else {
                                            self.cached_endorsement_ids.insert(endorsement.id);
                                            true
                                        }
                                    });
                                    let mut endorsements_storage = self.storage.clone_without_refs();
                                    endorsements_storage.store_endorsements(endorsements.clone());
                                    self.pool_controller.add_endorsements(endorsements_storage);
                                    self.internal_sender
                                        .send(InternalMessage::PropagateEndorsements((
                                            peer_id,
                                            endorsements,
                                        )))
                                        .unwrap();
                                }
                            }
                        }
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                },
                recv(self.receiver_ext) -> command => {
                    match command {
                        Ok(command) => {
                            println!("Received command: {:?}", command);
                        }
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                }
            }
        }
    }
}

pub fn start_retrieval_thread(
    receiver: Receiver<(PeerId, Vec<u8>)>,
    receiver_ext: Receiver<EndorsementHandlerCommand>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    internal_sender: Sender<InternalMessage>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver,
            receiver_ext,
            cached_endorsement_ids: PreHashSet::default(),
            pool_controller,
            storage,
            internal_sender,
        };
        retrieval_thread.run();
    })
}
