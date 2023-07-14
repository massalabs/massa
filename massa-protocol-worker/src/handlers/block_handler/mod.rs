use std::thread::JoinHandle;

use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_consensus_exports::ConsensusController;
use massa_metrics::MassaMetrics;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;
use massa_versioning::versioning::MipStore;

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
    AskForBlockInfo, BlockInfoReply, BlockMessageDeserializer, BlockMessageDeserializerArgs,
};

use super::{
    endorsement_handler::{
        cache::SharedEndorsementCache, commands_propagation::EndorsementHandlerPropagationCommand,
    },
    operation_handler::{
        cache::SharedOperationCache, commands_propagation::OperationHandlerPropagationCommand,
    },
    peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
};

pub struct BlockHandler {
    pub block_retrieval_thread: Option<(MassaSender<BlockHandlerRetrievalCommand>, JoinHandle<()>)>,
    pub block_propagation_thread:
        Option<(MassaSender<BlockHandlerPropagationCommand>, JoinHandle<()>)>,
}

impl BlockHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        active_connections: Box<dyn ActiveConnectionsTrait>,
        selector_controller: Box<dyn SelectorController>,
        consensus_controller: Box<dyn ConsensusController>,
        pool_controller: Box<dyn PoolController>,
        receiver_network: MassaReceiver<PeerMessageTuple>,
        sender_ext: MassaSender<BlockHandlerRetrievalCommand>,
        receiver_ext: MassaReceiver<BlockHandlerRetrievalCommand>,
        internal_receiver: MassaReceiver<BlockHandlerPropagationCommand>,
        internal_sender: MassaSender<BlockHandlerPropagationCommand>,
        sender_propagations_ops: MassaSender<OperationHandlerPropagationCommand>,
        sender_propagations_endorsements: MassaSender<EndorsementHandlerPropagationCommand>,
        peer_cmd_sender: MassaSender<PeerManagementCmd>,
        config: ProtocolConfig,
        endorsement_cache: SharedEndorsementCache,
        operation_cache: SharedOperationCache,
        cache: SharedBlockCache,
        storage: Storage,
        mip_store: MipStore,
        massa_metrics: MassaMetrics,
    ) -> Self {
        let block_retrieval_thread = start_retrieval_thread(
            active_connections.clone(),
            selector_controller,
            consensus_controller,
            pool_controller,
            receiver_network,
            receiver_ext,
            internal_sender.clone(),
            sender_propagations_ops,
            sender_propagations_endorsements,
            peer_cmd_sender.clone(),
            config.clone(),
            endorsement_cache,
            operation_cache,
            cache.clone(),
            storage.clone_without_refs(),
            mip_store,
            massa_metrics,
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
