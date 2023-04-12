use std::thread::JoinHandle;

use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use massa_pool_exports::PoolController;
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use peernet::peer_id::PeerId;

use super::{
    commands::OperationHandlerCommand,
    internal_messages::InternalMessage,
    messages::{OperationMessageDeserializer, OperationMessageDeserializerArgs},
};

pub struct RetrievalThread {
    receiver: Receiver<(PeerId, u64, Vec<u8>)>,
    receiver_ext: Receiver<OperationHandlerCommand>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    internal_sender: Sender<InternalMessage>,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Real values
        let mut operation_message_deserializer =
            OperationMessageDeserializer::new(OperationMessageDeserializerArgs {
                //TODO: Real value from config
                max_operations_prefix_ids: u32::MAX,
                max_operations: u32::MAX,
                max_datastore_value_length: u64::MAX,
                max_function_name_length: u16::MAX,
                max_parameters_size: u32::MAX,
                max_op_datastore_entry_count: u64::MAX,
                max_op_datastore_key_length: u8::MAX,
                max_op_datastore_value_length: u64::MAX,
            });
        loop {
            select! {
                recv(self.receiver) -> msg => {
                    match msg {
                        Ok((peer_id, message_id, message)) => {
                            operation_message_deserializer.set_message_id(message_id);
                            let (rest, message) = operation_message_deserializer
                                .deserialize::<DeserializeError>(&message)
                                .unwrap();
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                _ => todo!()
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
    receiver: Receiver<(PeerId, u64, Vec<u8>)>,
    receiver_ext: Receiver<OperationHandlerCommand>,
    pool_controller: Box<dyn PoolController>,
    storage: Storage,
    internal_sender: Sender<InternalMessage>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver,
            receiver_ext,
            pool_controller,
            storage,
            internal_sender,
        };
        retrieval_thread.run();
    })
}
