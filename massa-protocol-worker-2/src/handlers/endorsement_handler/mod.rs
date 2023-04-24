use std::thread::JoinHandle;

use crossbeam::channel::{Receiver, Sender};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::ProtocolConfig;
use massa_storage::Storage;
use peernet::network_manager::SharedActiveConnections;

use self::{
    cache::SharedEndorsementCache, commands_propagation::EndorsementHandlerCommand,
    propagation::start_propagation_thread, retrieval::start_retrieval_thread,
};

pub mod cache;
pub mod commands_propagation;
mod messages;
mod propagation;
mod retrieval;

pub(crate) use messages::{EndorsementMessage, EndorsementMessageSerializer};

use super::peer_handler::models::{PeerManagementCmd, PeerMessageTuple};

pub struct EndorsementHandler {
    pub endorsement_retrieval_thread: Option<JoinHandle<()>>,
    pub endorsement_propagation_thread: Option<JoinHandle<()>>,
}

impl EndorsementHandler {
    pub fn new(
        pool_controller: Box<dyn PoolController>,
        cache: SharedEndorsementCache,
        storage: Storage,
        config: ProtocolConfig,
        active_connections: SharedActiveConnections,
        receiver: Receiver<PeerMessageTuple>,
        local_sender: Sender<EndorsementHandlerCommand>,
        local_receiver: Receiver<EndorsementHandlerCommand>,
        sender_peer_cmd: Sender<PeerManagementCmd>,
    ) -> Self {
        let endorsement_retrieval_thread = start_retrieval_thread(
            receiver,
            local_sender,
            sender_peer_cmd,
            cache.clone(),
            pool_controller,
            config.clone(),
            storage.clone_without_refs(),
        );

        let endorsement_propagation_thread =
            start_propagation_thread(local_receiver, cache, config, active_connections);
        Self {
            endorsement_retrieval_thread: Some(endorsement_retrieval_thread),
            endorsement_propagation_thread: Some(endorsement_propagation_thread),
        }
    }

    pub fn stop(&mut self) {
        if let Some(thread) = self.endorsement_retrieval_thread.take() {
            thread.join().unwrap();
        }
        if let Some(thread) = self.endorsement_propagation_thread.take() {
            thread.join().unwrap();
        }
    }
}
