use std::thread::JoinHandle;

use crossbeam::channel::Receiver;
use massa_models::operation::OperationId;
use massa_protocol_exports_2::ProtocolConfig;
use peernet::network_manager::SharedActiveConnections;

use crate::messages::MessagesSerializer;

use super::{
    cache::SharedOperationCache, commands_propagation::OperationHandlerCommand,
    OperationMessageSerializer,
};

struct PropagationThread {
    internal_receiver: Receiver<OperationHandlerCommand>,
    //TODO: Add pruning
    operations_to_announce: Vec<OperationId>,
    config: ProtocolConfig,
    cache: SharedOperationCache,
}

impl PropagationThread {
    fn run(&mut self) {
        let operation_serializer = MessagesSerializer::new()
            .with_operation_message_serializer(OperationMessageSerializer::new());
        loop {
            match self.internal_receiver.recv() {
                Ok(internal_message) => match internal_message {
                    OperationHandlerCommand::AnnounceOperations(operations_ids) => {
                        self.operations_to_announce.extend(operations_ids);
                    }
                },
                Err(err) => {
                    println!("Error: {:?}", err);
                    return;
                }
            }
        }
    }
}

pub fn start_propagation_thread(
    internal_receiver: Receiver<OperationHandlerCommand>,
    active_connections: SharedActiveConnections,
    config: ProtocolConfig,
    cache: SharedOperationCache,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut propagation_thread = PropagationThread {
            internal_receiver,
            operations_to_announce: Vec::new(),
            config,
            cache,
        };
        propagation_thread.run();
    })
}
