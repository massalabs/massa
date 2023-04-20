use std::thread::JoinHandle;

use crossbeam::channel::Receiver;
use massa_protocol_exports_2::ProtocolConfig;
use peernet::network_manager::SharedActiveConnections;

use crate::messages::MessagesSerializer;

use super::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerCommand, BlockMessageSerializer,
};

pub struct PropagationThread {
    receiver: Receiver<BlockHandlerCommand>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
    active_connections: SharedActiveConnections,
    block_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        loop {
            match self.receiver.recv() {
                Ok(command) => {}
                Err(err) => {
                    println!("Error: {:?}", err)
                }
            }
        }
    }
}

pub fn start_propagation_thread(
    active_connections: SharedActiveConnections,
    receiver: Receiver<BlockHandlerCommand>,
    config: ProtocolConfig,
    cache: SharedBlockCache,
) -> JoinHandle<()> {
    //TODO: Here and everywhere add id to threads
    std::thread::spawn(move || {
        let block_serializer =
            MessagesSerializer::new().with_block_message_serializer(BlockMessageSerializer::new());
        let mut propagation_thread = PropagationThread {
            receiver,
            config,
            cache,
            active_connections,
            block_serializer,
        };
        propagation_thread.run();
    })
}
