use std::thread::JoinHandle;

use crossbeam::{channel::tick, select};
use massa_channel::{receiver::MassaReceiver, sender::MassaSender};
use massa_logging::massa_trace;
use massa_metrics::MassaMetrics;
use massa_models::{
    endorsement::SecureShareEndorsement,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    timeslots::get_block_slot_timestamp,
};
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{ProtocolConfig, ProtocolError};
use massa_serialization::{DeserializeError, Deserializer};
use massa_storage::Storage;
use massa_time::MassaTime;
use schnellru::{ByLength, LruMap};
use tracing::{debug, info, warn};

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
    receiver: MassaReceiver<PeerMessageTuple>,
    receiver_ext: MassaReceiver<EndorsementHandlerRetrievalCommand>,
    cache: SharedEndorsementCache,
    internal_sender: MassaSender<EndorsementHandlerPropagationCommand>,
    selector_controller: Box<dyn SelectorController>,
    pool_controller: Box<dyn PoolController>,
    config: ProtocolConfig,
    storage: Storage,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    metrics: MassaMetrics,
}

impl RetrievalThread {
    fn run(&mut self) {
        let endorsement_message_deserializer =
            EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
                thread_count: self.config.thread_count,
                max_length_endorsements: self.config.max_endorsements_per_message,
                endorsement_count: self.config.endorsement_count,
            });
        let tick_metrics = tick(self.metrics.tick_delay);

        loop {
            select! {
                recv(self.receiver) -> msg => {
                    info!("AURELIEN: Receiving message in endorsement handler");
                    self.receiver.update_metrics();
                    info!("AURELIEN: Receiving message in endorsement handler after metrics");
                    match msg {
                        Ok((peer_id, message)) => {
                            let (rest, message) = match endorsement_message_deserializer
                                .deserialize::<DeserializeError>(&message) {
                                Ok((rest, message)) => (rest, message),
                                Err(err) => {
                                    warn!("Error while deserializing message from peer {} err: {:?}", peer_id, err);
                                    continue;
                                }
                            };
                            if !rest.is_empty() {
                                println!("Error: message not fully consumed");
                                return;
                            }
                            match message {
                                EndorsementMessage::Endorsements(endorsements) => {
                                    debug!("Received endorsement message: Endorsement from {}", peer_id);
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
                            info!("Stop endorsement retrieval thread");
                            return;
                        }
                    }
                },
                recv(self.receiver_ext) -> msg => {
                    self.receiver_ext.update_metrics();
                    match msg {
                        Ok(msg) => {
                            match msg {
                                EndorsementHandlerRetrievalCommand::Stop => {
                                    info!("Stop endorsement retrieval thread");
                                    return;
                                }
                            }
                        }
                        Err(_) => {
                            info!("Stop endorsement retrieval thread");
                            return;
                        }
                    }
                },
                recv(tick_metrics) -> _ => {
                    // update metrics
                    let read = self.cache.read();
                    let count = read
                        .endorsements_known_by_peer
                        .iter()
                        .map(|(_peer_id, map)| map.len())
                        .sum();
                    self.metrics
                        .set_endorsements_cache_metrics(read.checked_endorsements.len(), count);
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
                if read_cache
                    .checked_endorsements
                    .peek(&endorsement_id)
                    .is_none()
                {
                    new_endorsements.insert(endorsement_id, endorsement);
                }
            }
        }

        // Batch signature verification
        // optimized signature verification
        verify_sigs_batch(
            &new_endorsements
                .values()
                .map(|endorsement| {
                    (
                        endorsement.compute_signed_hash(),
                        endorsement.signature,
                        endorsement.content_creator_pub_key,
                    )
                })
                .collect::<Vec<_>>(),
        )?;

        // Check PoS draws
        for endorsement in new_endorsements.values() {
            let selection = self
                .selector_controller
                .get_selection(endorsement.content.slot)?;
            let Some(address) = selection.endorsements.get(endorsement.content.index as usize) else {
                        return Err(ProtocolError::GeneralProtocolError(
                            format!(
                                "No selection on slot {} for index {}",
                                endorsement.content.slot, endorsement.content.index
                            )
                        ))
                    };
            if address != &endorsement.content_creator_address {
                return Err(ProtocolError::GeneralProtocolError(format!(
                    "Invalid endorsement: expected address {}, got {}",
                    address, endorsement.content_creator_address
                )));
            }
        }

        'write_cache: {
            let mut cache_write = self.cache.write();
            // add to verified signature cache
            for endorsement_id in endorsement_ids.iter() {
                cache_write.checked_endorsements.insert(*endorsement_id, ());
            }
            // add to known endorsements for source node.
            let Ok(endorsements) = cache_write
                .endorsements_known_by_peer
                .get_or_insert(from_peer_id.clone(), || {
                    LruMap::new(ByLength::new(
                        self.config
                            .max_node_known_endorsements_size
                            .try_into()
                            .expect("max_node_known_endorsements_size in config should be > 0"),
                    ))
                })
                .ok_or(()) else {
                    warn!("endorsements_known_by_peer limit reached");
                    break 'write_cache;
                };
            for endorsement_id in endorsement_ids.iter() {
                endorsements.insert(*endorsement_id, ());
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
            if let Err(err) = self.internal_sender.try_send(
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
            .try_send(PeerManagementCmd::Ban(vec![peer_id.clone()]))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start_retrieval_thread(
    receiver: MassaReceiver<PeerMessageTuple>,
    receiver_ext: MassaReceiver<EndorsementHandlerRetrievalCommand>,
    internal_sender: MassaSender<EndorsementHandlerPropagationCommand>,
    peer_cmd_sender: MassaSender<PeerManagementCmd>,
    cache: SharedEndorsementCache,
    selector_controller: Box<dyn SelectorController>,
    pool_controller: Box<dyn PoolController>,
    config: ProtocolConfig,
    storage: Storage,
    metrics: MassaMetrics,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name("protocol-endorsement-handler-retrieval".to_string())
        .spawn(move || {
            let mut retrieval_thread = RetrievalThread {
                receiver,
                receiver_ext,
                peer_cmd_sender,
                cache,
                internal_sender,
                selector_controller,
                pool_controller,
                config,
                storage,
                metrics,
            };
            retrieval_thread.run();
        })
        .expect("OS failed to start endorsement retrieval thread")
}
