use std::thread::JoinHandle;

use crossbeam::channel::{Receiver, Sender};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::ProtocolConfig;
use massa_storage::Storage;
use peernet::network_manager::SharedActiveConnections;

use self::{
    cache::SharedOperationCache, commands_propagation::OperationHandlerCommand,
    propagation::start_propagation_thread, retrieval::start_retrieval_thread,
};

pub mod cache;
pub mod commands_propagation;
mod messages;
mod propagation;
mod retrieval;

pub(crate) use messages::{OperationMessage, OperationMessageSerializer};

use super::peer_handler::models::{PeerManagementCmd, PeerMessageTuple};

pub struct OperationHandler {
    pub operation_retrieval_thread: Option<JoinHandle<()>>,
    pub operation_propagation_thread: Option<JoinHandle<()>>,
}

impl OperationHandler {
    pub fn new(
        pool_controller: Box<dyn PoolController>,
        storage: Storage,
        config: ProtocolConfig,
        cache: SharedOperationCache,
        active_connections: SharedActiveConnections,
        receiver_network: Receiver<PeerMessageTuple>,
        local_sender: Sender<OperationHandlerCommand>,
        local_receiver: Receiver<OperationHandlerCommand>,
        peer_cmd_sender: Sender<PeerManagementCmd>,
    ) -> Self {
        //TODO: Define bound channel
        let operation_retrieval_thread = start_retrieval_thread(
            receiver_network,
            pool_controller,
            storage.clone_without_refs(),
            config.clone(),
            cache.clone(),
            active_connections.clone(),
            local_sender,
            peer_cmd_sender,
        );

        let operation_propagation_thread =
            start_propagation_thread(local_receiver, active_connections, config, cache);
        Self {
            operation_retrieval_thread: Some(operation_retrieval_thread),
            operation_propagation_thread: Some(operation_propagation_thread),
        }
    }

    pub fn stop(&mut self) {
        if let Some(thread) = self.operation_retrieval_thread.take() {
            thread.join().unwrap();
        }
        if let Some(thread) = self.operation_propagation_thread.take() {
            thread.join().unwrap();
        }
    }
}
