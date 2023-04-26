use std::{num::NonZeroUsize, thread::JoinHandle};

use crossbeam::{
    channel::{Receiver, Sender},
    select,
};
use lru::LruCache;
use massa_logging::massa_trace;
use massa_models::{
    endorsement::SecureShareEndorsement,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    secure_share::Id,
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_protocol_exports_2::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use massa_time::MassaTime;
use peernet::peer_id::PeerId;
use tracing::warn;

use crate::{
    handlers::{
        endorsement_handler::messages::EndorsementMessage,
        peer_handler::models::{PeerManagementCmd, PeerMessageTuple},
    },
    sig_verifier::verify_sigs_batch,
};

use super::{
    cache::SharedEndorsementCache,
    commands_propagation::EndorsementHandlerPropagationCommand,
    commands_retrieval::EndorsementHandlerRetrievalCommand,
    messages::{EndorsementMessageDeserializer, EndorsementMessageDeserializerArgs},
};

pub struct RetrievalThread {
    receiver: Receiver<PeerMessageTuple>,
    receiver_ext: Receiver<EndorsementHandlerRetrievalCommand>,
    cache: SharedEndorsementCache,
    internal_sender: Sender<EndorsementHandlerPropagationCommand>,
    pool_controller: Box<dyn PoolController>,
    config: ProtocolConfig,
    storage: Storage,
    peer_cmd_sender: Sender<PeerManagementCmd>,
}

impl RetrievalThread {
    fn run(&mut self) {
        //TODO: Real values
        let mut endorsement_message_deserializer =
            EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
                thread_count: 32,
                max_length_endorsements: 10000,
                endorsement_count: 32,
            });
        loop {
            select! {
                recv(self.receiver) -> msg => {
                    match msg {
                        Ok((peer_id, message_id, message)) => {
                            endorsement_message_deserializer.set_message_id(message_id);
                            let (rest, message) = endorsement_message_deserializer
                                .deserialize::<DeserializeError>(&message)
                                .unwrap();
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                EndorsementMessage::Endorsements(endorsements) => {
                                    if let Err(err) =
                                        self.note_endorsements_from_peer(endorsements, &peer_id)
                                    {
                                        warn!(
                                            "peer {} sent us critically incorrect endorsements, \
                                            which may be an attack attempt by the remote node or a \
                                            loss of sync between us and the remote node. Err = {}",
                                            peer_id, err
                                        );
                                        if let Err(err) = self.ban_node(&peer_id) {
                                            warn!("Error while banning peer {} err: {:?}", peer_id, err);
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            println!("Stop endorsement retrieval thread");
                            return;
                        }
                    }
                },
                recv(self.receiver_ext) -> msg => {
                    match msg {
                        Ok(msg) => {
                            match msg {
                                EndorsementHandlerRetrievalCommand::Stop => {
                                    println!("Stop endorsement retrieval thread");
                                    return;
                                }
                            }
                        }
                        Err(_) => {
                            println!("Stop endorsement retrieval thread");
                            return;
                        }
                    }
                }
            }
        }
    }

    /// Note endorsements coming from a given node,
    /// and propagate them when they were received outside of a header.
    ///
    /// Caches knowledge of valid ones.
    ///
    /// Does not ban if the endorsement is invalid
    ///
    /// Checks performed:
    /// - Valid signature.
    pub(crate) fn note_endorsements_from_peer(
        &mut self,
        endorsements: Vec<SecureShareEndorsement>,
        from_peer_id: &PeerId,
    ) -> Result<(), ProtocolError> {
        massa_trace!("protocol.protocol_worker.note_endorsements_from_node", { "node": from_peer_id, "endorsements": endorsements});
        let length = endorsements.len();
        let mut new_endorsements = PreHashMap::with_capacity(length);
        let mut endorsement_ids = PreHashSet::with_capacity(length);
        for endorsement in endorsements.into_iter() {
            let endorsement_id = endorsement.id;
            endorsement_ids.insert(endorsement_id);
            // check endorsement signature if not already checked
            {
                let read_cache = self.cache.read();
                if !read_cache.checked_endorsements.contains(&endorsement_id) {
                    new_endorsements.insert(endorsement_id, endorsement);
                }
            }
        }

        // Batch signature verification
        // optimized signature verification
        verify_sigs_batch(
            &new_endorsements
                .iter()
                .map(|(endorsement_id, endorsement)| {
                    (
                        *endorsement_id.get_hash(),
                        endorsement.signature,
                        endorsement.content_creator_pub_key,
                    )
                })
                .collect::<Vec<_>>(),
        )?;

        {
            let mut cache_write = self.cache.write();
            // add to verified signature cache
            for endorsement_id in endorsement_ids.iter() {
                cache_write.checked_endorsements.put(*endorsement_id, ());
            }
            // add to known endorsements for source node.
            let endorsements = cache_write.endorsements_known_by_peer.get_or_insert_mut(
                from_peer_id.clone(),
                || {
                    LruCache::new(
                        NonZeroUsize::new(self.config.max_node_known_endorsements_size)
                            .expect("max_node_known_endorsements_size in config should be > 0"),
                    )
                },
            );
            for endorsement_id in endorsement_ids.iter() {
                endorsements.put(*endorsement_id, ());
            }
        }

        if !new_endorsements.is_empty() {
            let mut endorsements = self.storage.clone_without_refs();
            endorsements.store_endorsements(new_endorsements.into_values().collect());

            // Propagate endorsements
            // Propagate endorsements when the slot of the block they endorse isn't `max_endorsements_propagation_time` old.
            let mut endorsements_to_propagate = endorsements.clone();
            let endorsements_to_not_propagate = {
                let now = MassaTime::now()?;
                let read_endorsements = endorsements_to_propagate.read_endorsements();
                endorsements_to_propagate
                    .get_endorsement_refs()
                    .iter()
                    .filter_map(|endorsement_id| {
                        let slot_endorsed_block =
                            read_endorsements.get(endorsement_id).unwrap().content.slot;
                        let slot_timestamp = get_block_slot_timestamp(
                            self.config.thread_count,
                            self.config.t0,
                            self.config.genesis_timestamp,
                            slot_endorsed_block,
                        );
                        match slot_timestamp {
                            Ok(slot_timestamp) => {
                                if slot_timestamp
                                    .saturating_add(self.config.max_endorsements_propagation_time)
                                    < now
                                {
                                    Some(*endorsement_id)
                                } else {
                                    None
                                }
                            }
                            Err(_) => Some(*endorsement_id),
                        }
                    })
                    .collect()
            };
            endorsements_to_propagate.drop_endorsement_refs(&endorsements_to_not_propagate);
            if let Err(err) = self.internal_sender.send(
                EndorsementHandlerPropagationCommand::PropagateEndorsements(
                    endorsements_to_propagate,
                ),
            ) {
                warn!("Failed to send from retrieval thread of endorsement handler to propagation: {:?}", err);
            }
            // Add to pool
            self.pool_controller.add_endorsements(endorsements);
        }

        Ok(())
    }

    /// send a ban peer command to the peer handler
    fn ban_node(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .send(PeerManagementCmd::Ban(peer_id.clone()))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }
}

pub fn start_retrieval_thread(
    receiver: Receiver<PeerMessageTuple>,
    receiver_ext: Receiver<EndorsementHandlerRetrievalCommand>,
    internal_sender: Sender<EndorsementHandlerPropagationCommand>,
    peer_cmd_sender: Sender<PeerManagementCmd>,
    cache: SharedEndorsementCache,
    pool_controller: Box<dyn PoolController>,
    config: ProtocolConfig,
    storage: Storage,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut retrieval_thread = RetrievalThread {
            receiver,
            receiver_ext,
            peer_cmd_sender,
            cache,
            internal_sender,
            pool_controller,
            config,
            storage,
        };
        retrieval_thread.run();
    })
}
