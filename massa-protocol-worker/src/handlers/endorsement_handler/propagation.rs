use super::{
    cache::SharedEndorsementCache, commands_propagation::EndorsementHandlerPropagationCommand,
    is_endorsement_fresh, messages::EndorsementMessageSerializer, EndorsementMessage,
};
use crate::{messages::MessagesSerializer, wrap_network::ActiveConnectionsTrait};
use massa_channel::receiver::MassaReceiver;
use massa_models::endorsement::SecureShareEndorsement;
use massa_protocol_exports::ProtocolConfig;
use massa_storage::Storage;
use massa_time::MassaTime;
use std::thread::JoinHandle;
use tracing::{debug, info, log::warn};

// protocol-endorsement-handler-propagation
const THREAD_NAME: &str = "peh-propagation";
static_assertions::const_assert!(THREAD_NAME.len() < 16);

/// Endorsements need to propagate fast, so no buffering
struct PropagationThread {
    receiver: MassaReceiver<EndorsementHandlerPropagationCommand>,
    config: ProtocolConfig,
    cache: SharedEndorsementCache,
    active_connections: Box<dyn ActiveConnectionsTrait>,
    endorsement_serializer: MessagesSerializer,
}

impl PropagationThread {
    fn run(&mut self) {
        let mut next_message = None;
        loop {
            // get the next message to process
            let msg = match next_message.take() {
                Some(msg) => msg,
                None => match self.receiver.recv() {
                    Ok(msg) => msg,
                    Err(_) => {
                        info!("Stop endorsement propagation thread");
                        return;
                    }
                },
            };

            match msg {
                // endorsements to propagate
                EndorsementHandlerPropagationCommand::PropagateEndorsements(mut endorsements) => {
                    // also drain any remaining propagation messages that might have accumulated,
                    // within the per-round budget
                    next_message = drain_propagation_commands(
                        &self.receiver,
                        &mut endorsements,
                        self.config.max_endorsements_per_propagation_round,
                    );
                    // propagate the endorsements
                    self.propagate_endorsements(endorsements);
                }
                // stop the handler
                EndorsementHandlerPropagationCommand::Stop => {
                    info!("Stop endorsement propagation thread");
                    return;
                }
            }
        }
    }

    /// Perform propagation of endorsements to the connected peers
    fn propagate_endorsements(&mut self, endorsements: Storage) {
        // get all the endorsements to send
        let endorsements: Vec<_> = {
            let storage_lock = endorsements.read_endorsements();
            endorsements
                .get_endorsement_refs()
                .iter()
                .filter_map(|id| storage_lock.get(id).cloned())
                .collect()
        };

        // Drop endorsements whose inclusion slot is already too old to be worth relaying.
        // The retrieval thread already filters those out, but other callers reach us directly
        // (the endorsement factory, the gRPC `send_endorsements` endpoint), so applying the
        // policy here makes it hold for every source: obsolete endorsements neither consume
        // propagation bandwidth nor pollute `checked_endorsements`.
        let endorsements = filter_fresh_endorsements(endorsements, &self.config);
        if endorsements.is_empty() {
            return;
        }

        // get connected peers
        let peers_connected = self.active_connections.get_peer_ids_connected();

        // get a write lock on the cache
        let mut cache_write = self.cache.write();

        // mark that we have checked those endorsements
        for endorsement in &endorsements {
            cache_write.checked_endorsements.insert(endorsement.id, ());
        }

        // Add peers that potentially don't exist in cache and remove the ones that disconnected
        cache_write.update_cache(&peers_connected);

        // Propagate to peers
        'peer_loop: for peer_id in peers_connected {
            // write access to the cache of which endorsements are known by the peer
            let peer_knowledge = cache_write
                .endorsements_known_by_peer
                .get_mut(&peer_id)
                .expect("update_cache should have added connected peer to cache");

            // get endorsements that are not known by the peer
            let to_send: Vec<_> = endorsements
                .iter()
                .filter(|endorsement| peer_knowledge.peek(&endorsement.id).is_none())
                .collect();

            if to_send.is_empty() {
                // nothing to send to that peer, try the next one
                continue 'peer_loop;
            }

            // send by chunks
            for chunk in to_send.chunks(self.config.max_endorsements_per_message as usize) {
                if let Err(err) = self.active_connections.send_to_peer(
                    &peer_id,
                    &self.endorsement_serializer,
                    EndorsementMessage::Endorsements(chunk.iter().map(|&e| e.clone()).collect())
                        .into(),
                    false,
                ) {
                    warn!(
                        "could not send endorsements batch to node {}: {}",
                        peer_id, err
                    );
                    // try with next peer, this one is probably congested
                    continue 'peer_loop;
                }
                // sent successfully: mark peer as knowing the endorsements that were sent to it
                for endorsement in chunk {
                    peer_knowledge.insert(endorsement.id, ());
                }
            }
        }
    }
}

/// Keep only the endorsements that are still fresh enough to be propagated, logging the
/// discarded ones (see `is_endorsement_fresh`).
fn filter_fresh_endorsements(
    endorsements: Vec<SecureShareEndorsement>,
    config: &ProtocolConfig,
) -> Vec<SecureShareEndorsement> {
    let now = MassaTime::now();
    endorsements
        .into_iter()
        .filter(|endorsement| {
            let fresh = is_endorsement_fresh(endorsement, config, now);
            if !fresh {
                debug!(
                    "not propagating endorsement {}: its inclusion slot {} is too old",
                    endorsement.id, endorsement.content.slot
                );
            }
            fresh
        })
        .collect()
}

/// Merge the queued `PropagateEndorsements` commands into `endorsements` until the channel is
/// empty or `max_endorsements_per_propagation_round` endorsements have been accumulated.
///
/// Returns a message that was pulled from the channel but not consumed by this round (a
/// non-propagation command), which the caller must process next.
///
/// Bounding the drain keeps a single propagation round from growing unboundedly: without it a
/// sustained endorsement source could make the batch (and the per-peer work derived from it)
/// arbitrarily large. Endorsements left in the channel are not dropped, they are propagated by
/// the next loop iteration.
fn drain_propagation_commands(
    receiver: &MassaReceiver<EndorsementHandlerPropagationCommand>,
    endorsements: &mut Storage,
    max_endorsements_per_propagation_round: usize,
) -> Option<EndorsementHandlerPropagationCommand> {
    while endorsements.get_endorsement_refs().len() < max_endorsements_per_propagation_round {
        match receiver.try_recv() {
            // we got more endorsements to propagate: extend the buffer
            Ok(EndorsementHandlerPropagationCommand::PropagateEndorsements(new_endorsements)) => {
                endorsements.extend(new_endorsements);
            }
            // we grabbed a message that is not a propagation message, mark it for processing
            Ok(other_msg) => return Some(other_msg),
            // nothing left to merge
            Err(_) => break,
        }
    }
    None
}

pub fn start_propagation_thread(
    receiver: MassaReceiver<EndorsementHandlerPropagationCommand>,
    cache: SharedEndorsementCache,
    config: ProtocolConfig,
    active_connections: Box<dyn ActiveConnectionsTrait>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(THREAD_NAME.to_string())
        .spawn(move || {
            let endorsement_serializer = MessagesSerializer::new()
                .with_endorsement_message_serializer(EndorsementMessageSerializer::new());
            let mut propagation_thread = PropagationThread {
                receiver,
                config,
                active_connections,
                cache,
                endorsement_serializer,
            };
            propagation_thread.run();
        })
        .expect("OS failed to start endorsement propagation thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_channel::MassaChannel;
    use massa_hash::Hash;
    use massa_models::{
        block_id::BlockId,
        endorsement::{Endorsement, EndorsementSerializer, SecureShareEndorsement},
        secure_share::SecureShareContent,
        slot::Slot,
    };
    use massa_signature::KeyPair;

    /// Build an endorsement, `index` making it unique so that the storage does not deduplicate it
    fn endorsement(index: u32) -> SecureShareEndorsement {
        endorsement_at_slot(index, Slot::new(1, 0))
    }

    /// Build an endorsement for the given inclusion slot
    fn endorsement_at_slot(index: u32, slot: Slot) -> SecureShareEndorsement {
        let keypair = KeyPair::generate(0).unwrap();
        let content = Endorsement {
            slot,
            index,
            endorsed_block: BlockId::generate_from_hash(Hash::compute_from(b"block")),
        };
        Endorsement::new_verifiable(content, EndorsementSerializer::new(), &keypair, 0, None)
            .unwrap()
    }

    /// Build a storage sharing `root`'s objects and holding `count` distinct endorsements
    fn storage_with(root: &Storage, count: u32) -> Storage {
        let mut storage = root.clone_without_refs();
        storage.store_endorsements((0..count).map(endorsement).collect());
        storage
    }

    #[test]
    fn test_filter_fresh_endorsements_drops_obsolete_slots() {
        let config = ProtocolConfig {
            thread_count: 32,
            t0: MassaTime::from_millis(16000),
            max_endorsements_propagation_time: MassaTime::from_millis(32000),
            // genesis is far in the past, so the first slots are long obsolete
            genesis_timestamp: MassaTime::now().saturating_sub(MassaTime::from_millis(1_000_000)),
            ..Default::default()
        };

        let stale = endorsement_at_slot(0, Slot::new(1, 0));
        // a slot far in the future is not obsolete
        let fresh = endorsement_at_slot(1, Slot::new(100, 0));

        let kept = filter_fresh_endorsements(vec![stale.clone(), fresh.clone()], &config);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, fresh.id);
    }

    #[test]
    fn test_drain_propagation_commands_is_bounded() {
        let root = Storage::create_root();
        let (sender, receiver) = MassaChannel::new("test_drain_bounded".to_string(), None);
        for _ in 0..10 {
            sender
                .send(EndorsementHandlerPropagationCommand::PropagateEndorsements(
                    storage_with(&root, 3),
                ))
                .unwrap();
        }

        // budget of 5 endorsements: merging stops as soon as the budget is reached
        let mut endorsements = storage_with(&root, 3);
        assert!(drain_propagation_commands(&receiver, &mut endorsements, 5).is_none());
        assert_eq!(endorsements.get_endorsement_refs().len(), 6);
        // the commands over budget are left in the channel for the next round
        assert_eq!(receiver.len(), 9);
    }

    #[test]
    fn test_drain_propagation_commands_merges_everything_within_budget() {
        let root = Storage::create_root();
        let (sender, receiver) = MassaChannel::new("test_drain_full".to_string(), None);
        for _ in 0..3 {
            sender
                .send(EndorsementHandlerPropagationCommand::PropagateEndorsements(
                    storage_with(&root, 2),
                ))
                .unwrap();
        }

        let mut endorsements = root.clone_without_refs();
        assert!(drain_propagation_commands(&receiver, &mut endorsements, 1000).is_none());
        assert_eq!(endorsements.get_endorsement_refs().len(), 6);
        assert_eq!(receiver.len(), 0);
    }

    #[test]
    fn test_drain_propagation_commands_returns_non_propagation_command() {
        let root = Storage::create_root();
        let (sender, receiver) = MassaChannel::new("test_drain_stop".to_string(), None);
        sender
            .send(EndorsementHandlerPropagationCommand::PropagateEndorsements(
                storage_with(&root, 2),
            ))
            .unwrap();
        sender
            .send(EndorsementHandlerPropagationCommand::Stop)
            .unwrap();

        let mut endorsements = root.clone_without_refs();
        let next = drain_propagation_commands(&receiver, &mut endorsements, 1000);
        assert!(matches!(
            next,
            Some(EndorsementHandlerPropagationCommand::Stop)
        ));
        assert_eq!(endorsements.get_endorsement_refs().len(), 2);
    }
}
