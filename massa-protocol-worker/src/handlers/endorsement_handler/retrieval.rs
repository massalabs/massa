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
    endorsement_message_deserializer: EndorsementMessageDeserializer,
}

impl RetrievalThread {
    fn run(&mut self) {
        // regular interval ticks for metrics
        let tick_metrics = tick(self.metrics.tick_delay);

        loop {
            select! {
                recv(self.receiver) -> msg => {
                    self.receiver.update_metrics();
                    match msg {
                        Ok((peer_id, message)) => self.process_message(peer_id, message),
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
                    let cache_lock = self.cache.read();
                    let count = cache_lock
                        .endorsements_known_by_peer
                        .values()
                        .map(|v| v.len())
                        .sum();
                    self.metrics
                        .set_endorsements_cache_metrics(cache_lock.checked_endorsements.len(), count);
                }
            }
        }
    }

    /// Process incoming message
    fn process_message(&mut self, peer_id: PeerId, message: Vec<u8>) {
        let (rest, message) = match self
            .endorsement_message_deserializer
            .deserialize::<DeserializeError>(&message)
        {
            Ok((rest, message)) => (rest, message),
            Err(err) => {
                debug!(
                    "Error while deserializing message from peer {} err: {:?}",
                    peer_id, err
                );
                return;
            }
        };
        if !rest.is_empty() {
            debug!("Message not fully consumed");
            return;
        }
        match message {
            EndorsementMessage::Endorsements(endorsements) => {
                debug!("Received endorsement message: Endorsement from {}", peer_id);
                if let Err(err) = note_endorsements_from_peer(
                    endorsements,
                    &peer_id,
                    &self.cache,
                    self.selector_controller.as_ref(),
                    &self.storage,
                    &self.config,
                    &self.internal_sender,
                    self.pool_controller.as_mut(),
                ) {
                    warn!(
                        "peer {} sent us critically incorrect endorsements, \
                        which may be an attack attempt by the remote node or a \
                        loss of sync between us and the remote node. Err = {}",
                        peer_id, err
                    );
                    if let Err(err) = self.ban_peer(&peer_id) {
                        warn!("Error while banning peer {} err: {:?}", peer_id, err);
                    }
                }
            }
        }
    }

    /// send a ban peer command to the peer handler
    fn ban_peer(&mut self, peer_id: &PeerId) -> Result<(), ProtocolError> {
        massa_trace!("ban node from retrieval thread", { "peer_id": peer_id.to_string() });
        self.peer_cmd_sender
            .try_send(PeerManagementCmd::Ban(vec![peer_id.clone()]))
            .map_err(|err| ProtocolError::SendError(err.to_string()))
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn note_endorsements_from_peer(
    endorsements: Vec<SecureShareEndorsement>,
    from_peer_id: &PeerId,
    cache: &SharedEndorsementCache,
    selector_controller: &dyn SelectorController,
    storage: &Storage,
    config: &ProtocolConfig,
    endorsement_propagation_sender: &MassaSender<EndorsementHandlerPropagationCommand>,
    pool_controller: &mut dyn PoolController,
) -> Result<(), ProtocolError> {
    let mut new_endorsements = PreHashMap::with_capacity(endorsements.len());
    let mut all_endorsement_ids = PreHashSet::with_capacity(endorsements.len());

    // cache check
    {
        let cache_read = cache.read();
        for endorsement in endorsements.into_iter() {
            let endorsement_id = endorsement.id;
            all_endorsement_ids.insert(endorsement_id);

            // only consider the endorsement as new if we have not already checked it
            if cache_read
                .checked_endorsements
                .peek(&endorsement_id)
                .is_none()
            {
                new_endorsements.insert(endorsement_id, endorsement);
            }
        }
    }

    // Batch signature verification
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
        let selection = selector_controller
            .get_selection(endorsement.content.slot)?
            .endorsements;
        let Some(address) = selection.get(endorsement.content.index as usize) else {
                    return Err(ProtocolError::GeneralProtocolError(
                        format!(
                            "No selection on slot {} for index {}",
                            endorsement.content.slot, endorsement.content.index
                        )
                    ))
                };
        if address != &endorsement.content_creator_address {
            return Err(ProtocolError::GeneralProtocolError(format!(
                "Invalid endorsement producer selection: expected address {}, got {}",
                address, endorsement.content_creator_address
            )));
        }
    }

    {
        let mut cache_write = cache.write();

        // add to the cache of endorsements we have checked
        for endorsement_id in all_endorsement_ids.iter() {
            cache_write.insert_checked_endorsement(*endorsement_id);
        }

        // add to the cache of endorsements known by the source node
        cache_write.insert_peer_known_endorsements(
            from_peer_id,
            &all_endorsement_ids.iter().copied().collect::<Vec<_>>(),
        );
    }

    // From there we note new endorsements and propagate them

    // Filter out endorsements if they are too old (max age of the inclusion slot: `max_endorsements_propagation_time`)
    let now = MassaTime::now()?;
    new_endorsements.retain(|_id, endorsement| {
        match get_block_slot_timestamp(
            config.thread_count,
            config.t0,
            config.genesis_timestamp,
            endorsement.content.slot,
        ) {
            Ok(t) => t.saturating_add(config.max_endorsements_propagation_time) >= now,
            Err(_) => false,
        }
    });

    if new_endorsements.is_empty() {
        // no endorsements to note or propagate
        return Ok(());
    }

    // Store new endorsements
    let mut endorsement_store = storage.clone_without_refs();
    endorsement_store.store_endorsements(new_endorsements.into_values().collect());

    // Propagate to other peers
    if let Err(err) = endorsement_propagation_sender.try_send(
        EndorsementHandlerPropagationCommand::PropagateEndorsements(endorsement_store.clone()),
    ) {
        warn!(
            "Failed to send from retrieval thread of endorsement handler to propagation: {:?}",
            err
        );
    }

    // Add to pool
    pool_controller.add_endorsements(endorsement_store);

    Ok(())
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
    let endorsement_message_deserializer =
        EndorsementMessageDeserializer::new(EndorsementMessageDeserializerArgs {
            thread_count: config.thread_count,
            max_length_endorsements: config.max_endorsements_per_message,
            endorsement_count: config.endorsement_count,
        });
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
                endorsement_message_deserializer,
            };
            retrieval_thread.run();
        })
        .expect("OS failed to start endorsement retrieval thread")
}
