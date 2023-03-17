use std::thread::JoinHandle;

use crossbeam::channel::{unbounded, Receiver};
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use self::{propagation::start_propagation_thread, retrieval::start_retrieval_thread};

mod internal_messages;
mod messages;
mod propagation;
mod retrieval;

pub struct EndorsementHandler {
    pub endorsement_retrieval_thread: Option<JoinHandle<()>>,
    pub endorsement_propagation_thread: Option<JoinHandle<()>>,
}

impl EndorsementHandler {
    pub fn new(
        active_connections: SharedActiveConnections,
        receiver: Receiver<(PeerId, Vec<u8>)>,
    ) -> Self {
        //TODO: Define bound channel
        let (internal_sender, internal_receiver) = unbounded();
        let endorsement_retrieval_thread = start_retrieval_thread(receiver, internal_sender);

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
