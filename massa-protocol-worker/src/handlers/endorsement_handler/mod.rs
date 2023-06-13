use std::thread::JoinHandle;

use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_metrics::MassaMetrics;
use massa_pool_exports::PoolController;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;

use crate::wrap_network::ActiveConnectionsTrait;

use self::{
    cache::SharedEndorsementCache, commands_propagation::EndorsementHandlerPropagationCommand,
    commands_retrieval::EndorsementHandlerRetrievalCommand, propagation::start_propagation_thread,
    retrieval::start_retrieval_thread,
};

pub mod cache;
pub mod commands_propagation;
pub mod commands_retrieval;
mod messages;
mod propagation;
mod retrieval;

pub(crate) use messages::{EndorsementMessage, EndorsementMessageSerializer};

use super::peer_handler::models::{PeerManagementCmd, PeerMessageTuple};

pub struct EndorsementHandler {
    pub endorsement_retrieval_thread: Option<(
        MassaSender<EndorsementHandlerRetrievalCommand>,
        JoinHandle<()>,
    )>,
    pub endorsement_propagation_thread: Option<(
        MassaSender<EndorsementHandlerPropagationCommand>,
        JoinHandle<()>,
    )>,
}

impl EndorsementHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool_controller: Box<dyn PoolController>,
        cache: SharedEndorsementCache,
        storage: Storage,
        config: ProtocolConfig,
        active_connections: Box<dyn ActiveConnectionsTrait>,
        receiver: MassaReceiver<PeerMessageTuple>,
        sender_retrieval_ext: MassaSender<EndorsementHandlerRetrievalCommand>,
        receiver_retrieval_ext: MassaReceiver<EndorsementHandlerRetrievalCommand>,
        local_sender: MassaSender<EndorsementHandlerPropagationCommand>,
        local_receiver: MassaReceiver<EndorsementHandlerPropagationCommand>,
        sender_peer_cmd: MassaSender<PeerManagementCmd>,
        massa_metrics: MassaMetrics,
    ) -> Self {
        let endorsement_retrieval_thread = start_retrieval_thread(
            receiver,
            receiver_retrieval_ext,
            local_sender.clone(),
            sender_peer_cmd,
            cache.clone(),
            pool_controller,
            config.clone(),
            storage.clone_without_refs(),
            massa_metrics,
        );

        let endorsement_propagation_thread =
            start_propagation_thread(local_receiver, cache, config, active_connections);
        Self {
            endorsement_retrieval_thread: Some((
                sender_retrieval_ext,
                endorsement_retrieval_thread,
            )),
            endorsement_propagation_thread: Some((local_sender, endorsement_propagation_thread)),
        }
    }

    pub fn stop(&mut self) {
        if let Some((tx, thread)) = self.endorsement_retrieval_thread.take() {
            let _ = tx.send(EndorsementHandlerRetrievalCommand::Stop);
            thread.join().unwrap();
        }
        if let Some((tx, thread)) = self.endorsement_propagation_thread.take() {
            let _ = tx.send(EndorsementHandlerPropagationCommand::Stop);
            thread.join().unwrap();
        }
    }
}
