use std::thread::JoinHandle;

use crossbeam::{channel::{Receiver, Sender}, select};
use massa_protocol_exports_2::ProtocolConfig;
use massa_storage::Storage;

use crate::handlers::peer_handler::models::PeerMessageTuple;

use super::{commands_propagation::BlockHandlerCommand, commands_retrieval::BlockHandlerRetrievalCommand};

pub struct RetrievalThread {
    receiver_network: Receiver<PeerMessageTuple>,
    internal_sender: Sender<BlockHandlerCommand>,
    receiver: Receiver<BlockHandlerRetrievalCommand>,
    config: ProtocolConfig,
    storage: Storage,
}

impl RetrievalThread {
    fn run(&mut self) {
        loop {
            select! {
                recv(self.receiver_network) -> msg => {
                    
                },
                recv(self.receiver) -> msg => {
                }
            }
        }
    }
}

pub fn start_retrieval_thread(
    receiver_network: Receiver<PeerMessageTuple>,
    receiver: Receiver<BlockHandlerRetrievalCommand>,
    internal_sender: Sender<BlockHandlerCommand>,
    config: ProtocolConfig,
    storage: Storage,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver_network,
            receiver,
            internal_sender,
            config,
            storage,
        };
        retrieval_thread.run();
    })
}
