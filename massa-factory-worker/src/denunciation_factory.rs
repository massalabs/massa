use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::{
    block::BlockId,
    denunciation::{Denunciation, DenunciationSerializer},
    slot::Slot,
    timeslots::{get_block_slot_timestamp, get_closest_slot_to_timestamp},
    wrapped::WrappedContent,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::{
    sync::Arc,
    thread,
    time::Instant,
};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use tracing::{debug, warn};
use massa_models::endorsement::{EndorsementId, WrappedEndorsement};
use massa_models::prehash::{PreHashMap, PreHashSet};
use itertools::Itertools;
use massa_models::operation::{Operation, OperationSerializer, OperationType};
use crossbeam_channel::{select, unbounded, Receiver};
use massa_models::denunciation_interest::DenunciationInterest;

/// Structure gathering all elements needed by the factory thread
pub(crate) struct DenunciationFactoryWorker {
    cfg: FactoryConfig,
    wallet: Arc<RwLock<Wallet>>,
    channels: FactoryChannels,
    factory_receiver: Receiver<()>,
    half_t0: MassaTime,

    // TODO: BlockHeader, Denunciation
    items_of_interest_receiver: Receiver<DenunciationInterest>,

    denunciation_serializer: DenunciationSerializer, // FIXME: should be OperationSerializer?
    // seen_endorsements: PreHashMap<EndorsementId, WrappedEndorsement>,
    endorsements_by_slot_index: HashMap<(Slot, u32), Vec<WrappedEndorsement>>,
}

impl DenunciationFactoryWorker {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn(
        cfg: FactoryConfig,
        wallet: Arc<RwLock<Wallet>>,
        channels: FactoryChannels,
        factory_receiver: Receiver<()>,
        items_of_interest_receiver: Receiver<DenunciationInterest>
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("endorsement factory worker".into())
            .spawn(|| {
                let mut this = Self {
                    half_t0: cfg
                        .t0
                        .checked_div_u64(2)
                        .expect("could not compute half_t0"),
                    cfg,
                    wallet,
                    channels,
                    factory_receiver,
                    denunciation_serializer: DenunciationSerializer::new(),
                    // seen_endorsements: Default::default(),
                    endorsements_by_slot_index: Default::default(),
                    items_of_interest_receiver,
                };
                this.run();
            })
            .expect("could not spawn endorsement factory worker thread")
    }

    /*
    /// Process a slot: produce an endorsement at that slot if one of the managed keys is drawn.
    fn process_slot(&mut self, slot: Slot) {

        // FIXME: Which address to use?
        // Early return if nothing in Wallet (need a pub ley to sign new Operation)
        let keypairs = self.wallet
            .read()
            .get_full_wallet()
            .iter()
            .take(1)
            .map(|(addr, keypair)| keypair.clone())
            .collect::<Vec<KeyPair>>();

        if keypairs.is_empty() {
            return;
        }
        let keypair = keypairs.get(0).unwrap(); // safe to unwrap here

        // Find new endorsements (not yet seen) from Storage
        let mut endo_storage = self.channels.storage.clone_without_refs();
        let endorsement_indexes = endo_storage.read_endorsements();

        let new_endorsements: PreHashMap<EndorsementId, WrappedEndorsement> = endorsement_indexes
            .endorsements
            .iter()
            .filter(|(id, _)| self.seen_endorsements.contains_key(id) )
            .map(|(id, wrapped_endo)| (id.clone(), wrapped_endo.clone()))
            .collect();

        // Early return if nothing to do
        if new_endorsements.is_empty() {
            return;
        }

        let mut denunciations : Vec<Denunciation> = Vec::new();

        for (endo_id, wrapped_endo) in new_endorsements.into_iter() {

            // Store them in seen_endorsements
            // FIXME: what to do if insert fail?
            self.seen_endorsements.insert(endo_id, wrapped_endo.clone());

            // Store them in endorsements_by_slot_index
            let key = (wrapped_endo.content.slot, wrapped_endo.content.index);
            let res = self.endorsements_by_slot_index
                .entry(key)
                .and_modify(|h| {
                    h.insert(endo_id);
                });

            // create a denunciation
            match res {
                Entry::Occupied(eo) => {
                    let denunciations_ = eo
                        .get()
                        .iter()
                        .take(2) // only 1 denunciation for now
                        .tuples()
                        .map(|(e_id1, e_id2)| {
                            let e1 = self.seen_endorsements.get(e_id1).unwrap();
                            let e2 = self.seen_endorsements.get(e_id2).unwrap();
                            Denunciation::from_wrapped_endorsements(e1, e2)
                        })
                        .collect::<Vec<Denunciation>>();

                    denunciations.extend(denunciations_);
                }
                Entry::Vacant(_) => {}
            }
        }

        // Store WrappedOperation's (made from denunciations) it
        self.channels.storage.store_operations(
            denunciations
                .iter()
                .map(|de| {
                    // FIXME: fee & expire_period?
                    let op = Operation {
                        fee: Default::default(),
                        expire_period: 0,
                        op: OperationType::Denunciation { data: de.clone() },
                    };
                    Operation::new_wrapped(op,
                                           OperationSerializer::new(),
                                           keypair).unwrap()
                })
                .collect()
        );

        // Clean too old Denunciations (removing too old Slot)
        // TODO: on what criteria? Too old / Too much in the future?
        //


    }
    */

    fn process_new_endorsement(&mut self, wrapped_endorsement: WrappedEndorsement) {

        let key = (wrapped_endorsement.content.slot, wrapped_endorsement.content.index);
        let mut denunciations: Vec<Denunciation> = Vec::with_capacity(1);

        match self.endorsements_by_slot_index.entry(key) {
            Entry::Occupied(mut eo) => {
                let wrapped_endos = eo.get_mut();
                // Store at max 2 WrappedEndorsement's
                if wrapped_endos.len() == 1 {

                    wrapped_endos.push(wrapped_endorsement);

                    denunciations.extend(
                        wrapped_endos.iter()
                            .take(2)
                            .tuples()
                            .map(|(we1, we2)| {
                                Denunciation::from_wrapped_endorsements(we1, we2);
                            })
                            .collect()
                    );
                } else {
                    debug!("[De Factory][WrappedEndorsement] len: {}", wrapped_endos.len());
                }
            }
            Entry::Vacant(ev) => {
                ev.insert(vec![wrapped_endorsement]);
            }
        }

        // Send Denunciation in OperationPool
        let mut de_storage = self.channels.storage.clone_without_refs();
        de_storage.store_operations(denunciations
            .iter()
            .map(|de| {
                let op = Operation {
                    // Note: we do not care about fee & expire_period
                    //       as Denunciation will be 'stolen' by the block creator
                    fee: Default::default(),
                    expire_period: 0,
                    op: OperationType::Denunciation { data: de.clone() },
                };
                // TODO: no unwrap
                Operation::new_wrapped(op,
                                       OperationSerializer::new(),
                                       keypair).unwrap()
            })
            .collect()
        );

        self.channels.pool.add_operations(de_storage.clone());
        // And now send them to ProtocolWorker (for propagation)
        // FIXME: propagate_operations is async while propagate_endorsements is not?
        if let Err(err) = self.channels.protocol.propagate_operations(de_storage) {
            warn!("could not propagate denunciations to protocol: {}", err);
        }
    }

    /// main run loop of the endorsement creator thread
    fn run(&mut self) {
        loop {
            select! {
                recv(self.items_of_interest_receiver) -> items_ => {

                    match items_ {
                        Ok(DenunciationInterest::WrappedEndorsement(wrapped_endo)) => {
                            self.process_new_endorsement(wrapped_endo);
                        }
                        Ok(DenunciationInterest::WrappedOperations(ops)) => {
                            todo!()
                        }
                        Ok(DenunciationInterest::WrappedHeader(wrapped_header)) => {
                            todo!()
                        }
                        Err(e) => {
                            debug!("[De Factory] Error from items of interest receiver: {}", e);
                            break;
                        }
                    }
                }
                recv(self.factory_receiver) -> msg => {
                    if let Err(e) = msg {
                        debug!("[De Factory] Error from factory receiver: {}", e);
                    }
                    break;
                }
            }
        }
    }
}
