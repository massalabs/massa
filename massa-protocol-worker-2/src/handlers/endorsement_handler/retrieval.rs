use std::thread::JoinHandle;

use crossbeam::channel::{Receiver, Sender};
use massa_models::{endorsement::EndorsementId, prehash::PreHashSet};
use massa_serialization::{DeserializeError, Deserializer};
use peernet::peer_id::PeerId;

use crate::handlers::endorsement_handler::messages::EndorsementMessage;

use super::{
    internal_messages::InternalMessage,
    messages::{EndorsementMessageDeserializer, EndorsementMessageDeserializerArgs},
};

pub struct RetrievalThread {
    cached_endorsement_ids: PreHashSet<EndorsementId>,
}

pub fn start_retrieval_thread(
    receiver: Receiver<(PeerId, Vec<u8>)>,
    internal_sender: Sender<InternalMessage>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            cached_endorsement_ids: PreHashSet::default(),
        };
        //TODO: Real values
        let endorsement_message_deserializer =
            EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
                thread_count: 32,
                max_length_endorsements: 10000,
                endorsement_count: 32,
            });
        loop {
            match receiver.recv() {
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
                                if retrieval_thread
                                    .cached_endorsement_ids
                                    .contains(&endorsement.id)
                                {
                                    false
                                } else {
                                    retrieval_thread
                                        .cached_endorsement_ids
                                        .insert(endorsement.id);
                                    true
                                }
                            });
                            internal_sender
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
        }
    })
}
