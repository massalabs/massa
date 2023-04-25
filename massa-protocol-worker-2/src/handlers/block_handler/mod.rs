use std::thread::JoinHandle;

use crossbeam::channel::{Receiver, Sender};
use massa_consensus_exports::ConsensusController;
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::ProtocolConfig;
use massa_storage::Storage;

use crate::wrap_network::ActiveConnectionsTrait;

use self::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerPropagationCommand,
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

#[cfg(feature = "testing")]
pub use messages::{
    AskForBlocksInfo, BlockInfoReply, BlockMessageDeserializer, BlockMessageDeserializerArgs,
};

use super::{
    endorsement_handler::cache::SharedEndorsementCache,
    operation_handler::cache::SharedOperationCache,
    peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
};

pub struct BlockHandler {
    pub block_retrieval_thread: Option<(Sender<BlockHandlerRetrievalCommand>, JoinHandle<()>)>,
    pub block_propagation_thread: Option<(Sender<BlockHandlerPropagationCommand>, JoinHandle<()>)>,
}

impl BlockHandler {
    pub fn new(
        active_connections: Box<dyn ActiveConnectionsTrait>,
        consensus_controller: Box<dyn ConsensusController>,
        pool_controller: Box<dyn PoolController>,
        receiver_network: Receiver<PeerMessageTuple>,
        sender_ext: Sender<BlockHandlerRetrievalCommand>,
        receiver_ext: Receiver<BlockHandlerRetrievalCommand>,
        internal_receiver: Receiver<BlockHandlerPropagationCommand>,
        internal_sender: Sender<BlockHandlerPropagationCommand>,
        peer_cmd_sender: Sender<PeerManagementCmd>,
        config: ProtocolConfig,
        endorsement_cache: SharedEndorsementCache,
        operation_cache: SharedOperationCache,
        cache: SharedBlockCache,
        storage: Storage,
    ) -> Self {
        let block_retrieval_thread = start_retrieval_thread(
            active_connections.clone(),
            consensus_controller,
            pool_controller,
            receiver_network,
            receiver_ext,
            internal_sender.clone(),
            peer_cmd_sender.clone(),
            config.clone(),
            endorsement_cache,
            operation_cache,
            cache.clone(),
            storage,
        );
        let block_propagation_thread = start_propagation_thread(
            active_connections,
            internal_receiver,
            peer_cmd_sender,
            config,
            cache,
        );
        Self {
            block_retrieval_thread: Some((sender_ext, block_retrieval_thread)),
            block_propagation_thread: Some((internal_sender, block_propagation_thread)),
        }
    }

    pub fn stop(&mut self) {
        if let Some((tx, thread)) = self.block_retrieval_thread.take() {
            let _ = tx.send(BlockHandlerRetrievalCommand::Stop);
            thread.join().unwrap();
        }
        if let Some((tx, thread)) = self.block_propagation_thread.take() {
            let _ = tx.send(BlockHandlerPropagationCommand::Stop);
            thread.join().unwrap();
        }
    }
}
