use std::thread::JoinHandle;

use crossbeam::channel::{Receiver, Sender};
use massa_protocol_exports_2::ProtocolConfig;
use massa_storage::Storage;
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use self::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerCommand,
    commands_retrieval::BlockHandlerRetrievalCommand, propagation::start_propagation_thread,
    retrieval::start_retrieval_thread,
};

pub mod cache;
pub mod commands_propagation;
pub mod commands_retrieval;
mod messages;
mod propagation;
mod retrieval;

pub(crate) use messages::{BlockMessage, BlockMessageSerializer};

pub struct BlockHandler {
    pub block_retrieval_thread: Option<JoinHandle<()>>,
    pub block_propagation_thread: Option<JoinHandle<()>>,
}

impl BlockHandler {
    pub fn new(
        active_connections: SharedActiveConnections,
        receiver_network: Receiver<(PeerId, u64, Vec<u8>)>,
        receiver_ext: Receiver<BlockHandlerRetrievalCommand>,
        internal_receiver: Receiver<BlockHandlerCommand>,
        internal_sender: Sender<BlockHandlerCommand>,
        config: ProtocolConfig,
        cache: SharedBlockCache,
        storage: Storage,
    ) -> Self {
        let block_retrieval_thread = start_retrieval_thread(
            active_connections,
            receiver_network,
            receiver_ext,
            internal_sender,
            config,
            cache,
            storage,
        );
        let block_propagation_thread =
            start_propagation_thread(active_connections, internal_receiver, config, cache);
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
