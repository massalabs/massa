use std::thread::JoinHandle;

use crossbeam::channel::{unbounded, Receiver};
use massa_pool_exports::PoolController;
use massa_storage::Storage;
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use self::{
    commands::EndorsementHandlerCommand, propagation::start_propagation_thread,
    retrieval::start_retrieval_thread,
};

pub mod commands;
mod internal_messages;
mod messages;
mod propagation;
mod retrieval;

pub(crate) use messages::{EndorsementMessage, EndorsementMessageSerializer};


pub struct EndorsementHandler {
    pub endorsement_retrieval_thread: Option<JoinHandle<()>>,
    pub endorsement_propagation_thread: Option<JoinHandle<()>>,
}

impl EndorsementHandler {
    pub fn new(
        pool_controller: Box<dyn PoolController>,
        storage: Storage,
        active_connections: SharedActiveConnections,
        receiver: Receiver<(PeerId, u64, Vec<u8>)>,
        receiver_ext: Receiver<EndorsementHandlerCommand>,
    ) -> Self {
        //TODO: Define bound channel
        let (internal_sender, internal_receiver) = unbounded();
        let endorsement_retrieval_thread = start_retrieval_thread(
            receiver,
            receiver_ext,
            pool_controller,
            storage,
            internal_sender,
        );

        let endorsement_propagation_thread =
            start_propagation_thread(internal_receiver, active_connections);
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
