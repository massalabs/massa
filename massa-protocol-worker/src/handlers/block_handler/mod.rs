use std::thread::JoinHandle;

use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_consensus_exports::ConsensusController;
use massa_metrics::MassaMetrics;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;
use massa_versioning::versioning::MipStore;
use tracing::warn;

use crate::wrap_network::ActiveConnectionsTrait;

use self::{
    cache::SharedBlockCache, commands_propagation::BlockHandlerPropagationCommand,
    commands_retrieval::BlockHandlerRetrievalCommand, propagation::start_propagation_thread,
    retrieval::start_retrieval_thread,
};

pub mod cache;
pub mod commands_propagation;
pub mod commands_retrieval;
pub mod messages;
mod propagation;
mod retrieval;

pub(crate) use messages::{BlockMessage, BlockMessageSerializer};

#[cfg(test)]
pub use messages::{AskForBlockInfo, BlockInfoReply};

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
        // Signal both threads to stop *before* joining any of them, so a panic
        // in (or a failed join of) one thread cannot prevent the other from
        // being told to stop and joined.
        if let Some((tx, _)) = self.block_retrieval_thread.as_ref() {
            let _ = tx.send(BlockHandlerRetrievalCommand::Stop);
        }
        if let Some((tx, _)) = self.block_propagation_thread.as_ref() {
            let _ = tx.send(BlockHandlerPropagationCommand::Stop);
        }

        // Join both, tolerating an already-panicked thread (`join()` returns
        // `Err`) so shutdown of the other thread still completes instead of
        // aborting `stop()` via `unwrap()`.
        if let Some((_, thread)) = self.block_retrieval_thread.take() {
            join_logging_panic(thread, "block retrieval");
        }
        if let Some((_, thread)) = self.block_propagation_thread.take() {
            join_logging_panic(thread, "block propagation");
        }
    }
}

/// Join a handler thread, logging (instead of propagating) the case where the
/// thread had already panicked, so one panicked thread does not abort the
/// shutdown of the others.
fn join_logging_panic(thread: JoinHandle<()>, name: &str) {
    if thread.join().is_err() {
        warn!("{} thread panicked before/at shutdown", name);
    }
}

#[cfg(test)]
mod tests {
    use super::join_logging_panic;

    #[test]
    fn join_logging_panic_tolerates_a_panicked_thread() {
        // A thread that panics must not make `join_logging_panic` itself panic,
        // so a sibling thread's shutdown can still proceed.
        let panicker = std::thread::Builder::new()
            .name("test-panicker".into())
            .spawn(|| panic!("intentional test panic"))
            .unwrap();
        join_logging_panic(panicker, "test-panicker");

        let normal = std::thread::spawn(|| {});
        join_logging_panic(normal, "test-normal");
    }
}
