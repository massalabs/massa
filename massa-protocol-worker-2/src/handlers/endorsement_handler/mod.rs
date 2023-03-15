use std::thread::JoinHandle;

use crossbeam::channel::{unbounded, Receiver, Sender};
use massa_serialization::{DeserializeError, Deserializer};
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use self::messages::{
    EndorsementMessageDeserializer, EndorsementMessageDeserializerArgs,
    EndorsementMessageSerializer,
};

mod messages;

pub struct EndorsementHandler {
    pub endorsement_retrieval_thread: Option<JoinHandle<()>>,
    pub endorsement_propagation_thread: Option<JoinHandle<()>>,
}

impl EndorsementHandler {
    pub fn new(
        active_connections: SharedActiveConnections,
        receiver: Receiver<(PeerId, Vec<u8>)>,
    ) -> Self {
        //TODO: Define real data
        let (_internal_sender, internal_receiver): (Sender<()>, Receiver<()>) = unbounded();
        let endorsement_retrieval_thread = std::thread::spawn(move || {
            //TODO: Real values
            let endorsement_message_deserializer =
                EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
                    thread_count: 32,
                    max_length_endorsements: 10000,
                    endorsement_count: 32,
                });
            //TODO: Real logic
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
                        println!("Received message from {:?}: {:?}", peer_id, message);
                    }
                    Err(err) => {
                        println!("Error: {:?}", err);
                        return;
                    }
                }
            }
        });

        let endorsement_propagation_thread = std::thread::spawn({
            let _active_connections = active_connections.clone();
            move || {
                let _endorsement_message_serializer = EndorsementMessageSerializer::new();
                //TODO: Real logic
                loop {
                    match internal_receiver.recv() {
                        Ok(_data) => {
                            // Example to send data
                            // {
                            //     let active_connections = active_connections.read();
                            //     for (peer_id, connection) in active_connections.iter() {
                            //         println!("Sending message to {:?}", peer_id);
                            //         let buf = Vec::new();
                            //         endorsement_message_serializer.serialize(&data, &mut buf).unwrap();
                            //         connection.send_message(&buf);
                            //     }
                            // }
                            println!("Received message");
                        }
                        Err(err) => {
                            println!("Error: {:?}", err);
                            return;
                        }
                    }
                }
            }
        });
        Self {
            endorsement_retrieval_thread: Some(endorsement_retrieval_thread),
            endorsement_propagation_thread: Some(endorsement_propagation_thread),
        }
    }

    pub fn stop(&mut self) {
        if let Some(thread) = self.endorsement_retrieval_thread.take() {
            thread.join().unwrap();
        }
        if let Some(thread) = self.endorsement_propagation_thread.take() {
            thread.join().unwrap();
        }
    }
}
