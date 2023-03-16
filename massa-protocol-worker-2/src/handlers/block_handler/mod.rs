use std::thread::JoinHandle;

use crossbeam::channel::{unbounded, Receiver, Sender};
use massa_serialization::{DeserializeError, Deserializer};
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use self::messages::{
    BlockMessageDeserializer, BlockMessageDeserializerArgs, BlockMessageSerializer,
};

mod messages;

pub struct BlockHandler {
    pub block_retrieval_thread: Option<JoinHandle<()>>,
    pub block_propagation_thread: Option<JoinHandle<()>>,
}

impl BlockHandler {
    pub fn new(
        active_connections: SharedActiveConnections,
        receiver: Receiver<(PeerId, Vec<u8>)>,
    ) -> Self {
        //TODO: Define real data
        let (_internal_sender, internal_receiver): (Sender<()>, Receiver<()>) = unbounded();
        let block_retrieval_thread = std::thread::spawn(move || {
            //TODO: Real values
            let block_message_deserializer =
                BlockMessageDeserializer::new(BlockMessageDeserializerArgs {
                    thread_count: 32,
                    endorsement_count: 32,
                    block_infos_length_max: 100000,
                    max_operations_per_block: 100000,
                    max_datastore_value_length: 100000,
                    max_function_name_length: 10000,
                    max_op_datastore_entry_count: 100000,
                    max_op_datastore_key_length: 200,
                    max_op_datastore_value_length: 100000,
                    max_parameters_size: 100000,
                });
            //TODO: Real logic
            loop {
                match receiver.recv() {
                    Ok((peer_id, message)) => {
                        let (rest, message) = block_message_deserializer
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

        let block_propagation_thread = std::thread::spawn({
            let _active_connections = active_connections.clone();
            move || {
                let _block_message_serializer = BlockMessageSerializer::new();
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
                            //         block_message_serializer.serialize(&data, &mut buf).unwrap();
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
            block_retrieval_thread: Some(block_retrieval_thread),
            block_propagation_thread: Some(block_propagation_thread),
        }
    }

    pub fn stop(&mut self) {
        if let Some(thread) = self.block_retrieval_thread.take() {
            thread.join().unwrap();
        }
        if let Some(thread) = self.block_propagation_thread.take() {
            thread.join().unwrap();
        }
    }
}
