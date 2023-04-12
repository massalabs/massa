use std::{collections::HashMap, thread::JoinHandle};

use crossbeam::channel::Receiver;
use massa_models::{endorsement::EndorsementId, prehash::PreHashSet};
use peernet::{network_manager::SharedActiveConnections, peer_id::PeerId};

use crate::messages::MessagesSerializer;

use super::{internal_messages::InternalMessage, OperationMessageSerializer};

struct PropagationThread {
    //TODO: Add pruning
    cache_by_peer: HashMap<PeerId, PreHashSet<EndorsementId>>,
}

pub fn start_propagation_thread(
    internal_receiver: Receiver<InternalMessage>,
    active_connections: SharedActiveConnections,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let endorsement_serializer = MessagesSerializer::new()
            .with_operation_message_serializer(OperationMessageSerializer::new());
        let mut propagation_thread = PropagationThread {
            cache_by_peer: HashMap::new(),
        };
        loop {
            match internal_receiver.recv() {
                Ok(internal_message) => match internal_message {
                    _ => todo!(),
                },
                Err(err) => {
                    println!("Error: {:?}", err);
                    return;
                }
            }
        }
    })
}
