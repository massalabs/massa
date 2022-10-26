use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::{
    denunciation::{Denunciation},
    slot::Slot,
    wrapped::WrappedContent,
};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use std::{
    thread,
};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use tracing::debug;
use massa_models::endorsement::WrappedEndorsement;
use itertools::Itertools;
use massa_models::operation::{Operation, OperationSerializer, OperationType, WrappedOperation};
use crossbeam_channel::{select, Receiver};
use massa_models::block::WrappedHeader;
use massa_models::denunciation::DenunciationProof;
use massa_models::denunciation_interest::DenunciationInterest;
use massa_models::timeslots::get_closest_slot_to_timestamp;

// const DENUNCIATION_EXPIRE_CYCLE_DELTA_EXPIRE_COUNT: u64 = 3;
const ENDORSEMENT_DENUNCIATION_CACHE_MAX_SIZE: usize = 4096;
const ENDORSEMENT_BY_CACHE_MAX_SIZE: usize = 4096;
const BLOCK_HEADER_DENUNCIATION_CACHE_MAX_SIZE: usize = 4096;
const BLOCK_HEADER_BY_CACHE_MAX_SIZE: usize = 4096;

/// Structure gathering all elements needed by the factory thread
pub(crate) struct DenunciationFactoryWorker {

    /// cfg
    cfg: FactoryConfig,

    /// Access to pool, selector, storage, ...
    channels: FactoryChannels,

    /// Queue to receive stop event
    factory_receiver: Receiver<()>,

    /// Queue to receive DenunciationInterest
    items_of_interest_receiver: Receiver<DenunciationInterest>,

    /// Key to generate wrapped operation
    genesis_key: KeyPair,

    /// Internal storage for potential future endorsement denunciation production
    endorsements_by_slot_index: HashMap<(Slot, u32), Vec<WrappedEndorsement>>,
    /// Internal storage for potential future block denunciation production
    block_header_by_slot: HashMap<Slot, Vec<WrappedHeader>>,

    /// Cache to avoid processing several time the same endorsement denunciation
    seen_endorsement_denunciation: HashSet<(Slot, u32)>,

    /// Cache to avoid processing several time the same block denunciation
    seen_block_header_denunciation: HashSet<Slot>,

    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
}

impl DenunciationFactoryWorker {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn(
        cfg: FactoryConfig,
        channels: FactoryChannels,
        factory_receiver: Receiver<()>,
        items_of_interest_receiver: Receiver<DenunciationInterest>,
        genesis_key: KeyPair
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("Denunciation factory worker".into())
            .spawn(|| {

                let thread_count: usize = cfg.thread_count.into();
                let mut this = Self {
                    cfg,
                    channels,
                    factory_receiver,
                    endorsements_by_slot_index: Default::default(),
                    block_header_by_slot: Default::default(),
                    seen_endorsement_denunciation: Default::default(),
                    seen_block_header_denunciation: Default::default(),
                    items_of_interest_receiver,
                    genesis_key,
                    last_cs_final_periods: vec![0u64; thread_count]
                };
                this.run();
            })
            .expect("could not spawn endorsement factory worker thread")
    }

    /// Gets the next slot
    fn get_next_slot(&self) -> Slot {
        // get delayed time
        let now = MassaTime::now(self.cfg.clock_compensation_millis)
                .expect("could not get current time");

        // get closest slot according to the current absolute time
        get_closest_slot_to_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            now,
        )
    }

    fn process_new_endorsement(&mut self, wrapped_endorsement: WrappedEndorsement) {

        let key = (wrapped_endorsement.content.slot, wrapped_endorsement.content.index);
        if self.seen_endorsement_denunciation.contains(&key) {
            return;
        }

        if self.seen_endorsement_denunciation.len() > ENDORSEMENT_DENUNCIATION_CACHE_MAX_SIZE ||
            self.endorsements_by_slot_index.len() > ENDORSEMENT_BY_CACHE_MAX_SIZE {
            // TODO: Remove this for Testnet 17
            //       Debug feature for Testnet 16.x only
            debug!("[De Factory] new endorsements: cache full: {}, {}",
                self.seen_endorsement_denunciation.len(),
                self.endorsements_by_slot_index.len());
            return;
        }

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
                                Denunciation::from((we1, we2))
                            })
                            .collect::<Vec<Denunciation>>()
                    );
                } else {
                    debug!("[De Factory][WrappedEndorsement] len: {}", wrapped_endos.len());
                }
            }
            Entry::Vacant(ev) => {
                ev.insert(vec![wrapped_endorsement]);
            }
        }

        // Create Operation from our denunciations
        let wrapped_operations: Result<Vec<WrappedOperation>, _> = denunciations
            .iter()
            .map(|de| {
                let op = Operation {
                    // Note: we do not care about fee & expire_period
                    //       as Denunciation will be 'stolen' by the block creator
                    fee: Default::default(),
                    expire_period: 0,
                    op: OperationType::Denunciation { data: de.clone() },
                };
                Operation::new_wrapped(op,
                                       OperationSerializer::new(),
                                       &self.genesis_key)
            })
            .collect();

        if let Err(e) = wrapped_operations {
            // Should never happen
            panic!("Cannot build wrapped operations for new denunciations: {}", e);
        }

        // Add to operation pool
        self.channels.storage.store_operations(wrapped_operations.unwrap());

        // TODO: enable this for testnet 17
        // let mut de_storage = self.channels.storage.clone_without_refs();
        // de_storage.store_operations(wrapped_operations.unwrap());
        // self.channels.pool.add_operations(de_storage.clone());
        debug!("[De Factory] Should add Denunciation operations to pool...");
        // TODO: enable this for testnet 17
        // And now send them to ProtocolWorker (for propagation)
        /*
        if let Err(err) = self.channels.protocol.propagate_operations_sync(de_storage) {
            warn!("could not propagate denunciations to protocol: {}", err);
        }
        */
        debug!("[De Factory] Should propagate Denunciation operations...");

        self.cleanup_cache();
    }

    fn process_new_block_header(&mut self, wrapped_header: WrappedHeader) {

        let key = wrapped_header.content.slot;

        if self.seen_block_header_denunciation.contains(&key) {
            return;
        }

        if self.seen_block_header_denunciation.len() > BLOCK_HEADER_DENUNCIATION_CACHE_MAX_SIZE ||
            self.block_header_by_slot.len() > BLOCK_HEADER_BY_CACHE_MAX_SIZE {
            // TODO: Remove this for Testnet 17
            //       Debug feature for Testnet 16.x only
            debug!("[De Factory] new block header: cache full: {}, {}",
                self.seen_block_header_denunciation.len(),
                self.block_header_by_slot.len());
            return;
        }

        let mut denunciations: Vec<Denunciation> = Vec::with_capacity(1);

        match self.block_header_by_slot.entry(key) {
            Entry::Occupied(mut eo) => {
                let wrapped_headers = eo.get_mut();

                // Store at max 2 WrappedHeader's
                if wrapped_headers.len() == 1 {

                    wrapped_headers.push(wrapped_header);

                    denunciations.extend(
                        wrapped_headers.iter()
                            .take(2)
                            .tuples()
                            .map(|(wh1, wh2)| {
                                Denunciation::from((wh1, wh2))
                            })
                            .collect::<Vec<Denunciation>>()
                    );
                } else {
                    debug!("[De Factory][WrappedHeader] len: {}", wrapped_headers.len());
                }
            }
            Entry::Vacant(ev) => {
                ev.insert(vec![wrapped_header]);
            }
        }

        // Create Operation from our denunciations
        let wrapped_operations: Result<Vec<WrappedOperation>, _> = denunciations
            .iter()
            .map(|de| {
                let op = Operation {
                    // Note: we do not care about fee & expire_period
                    //       as Denunciation will be 'stolen' by the block creator
                    fee: Default::default(),
                    expire_period: 0,
                    op: OperationType::Denunciation { data: de.clone() },
                };
                Operation::new_wrapped(op,
                                       OperationSerializer::new(),
                                       &self.genesis_key)
            })
            .collect();

        if let Err(e) = wrapped_operations {
            // Should never happen
            panic!("Cannot build wrapped operations for new denunciations: {}", e);
        }

        // Add to operation pool
        self.channels.storage.store_operations(wrapped_operations.unwrap());

        // TODO: enable this for testnet 17
        // let mut de_storage = self.channels.storage.clone_without_refs();
        // de_storage.store_operations(wrapped_operations.unwrap());
        // self.channels.pool.add_operations(de_storage.clone());
        debug!("[De Factory] Should add Denunciation operations to pool...");
        // TODO: enable this for testnet 17
        // And now send them to ProtocolWorker (for propagation)
        /*
        if let Err(err) = self.channels.protocol.propagate_operations_sync(de_storage) {
            warn!("could not propagate denunciations to protocol: {}", err);
        }
        */
        debug!("[De Factory] Should propagate Denunciation operations...");

        self.cleanup_cache();
    }

    fn process_new_ops(&mut self, wrapped_operations: Vec<WrappedOperation>) {

        // Keep only Operation(Denunciation) && update 'seen hashset'
        wrapped_operations
            .iter()
            .for_each(|wop| {
                if let OperationType::Denunciation { data: de } = &wop.content.op {
                    let next_slot = self.get_next_slot();
                    if !is_expired_for_denunciation(&de.slot, &next_slot,
                                                         &self.last_cs_final_periods,
                                                    self.cfg.periods_per_cycle, self.cfg.denunciation_expire_cycle_delta) {
                        match de.proof.as_ref() {
                            DenunciationProof::Endorsement(ed) => {
                                self.seen_endorsement_denunciation.insert((de.slot, ed.index));
                            }
                            DenunciationProof::Block(_) => {
                                self.seen_block_header_denunciation.insert(de.slot);
                            }
                        }
                    }
                }
            })
    }


    fn cleanup_cache(&mut self) {

        let next_slot = self.get_next_slot();

        self.endorsements_by_slot_index.retain(|(slot, _index), _| {
            !is_expired_for_denunciation(slot, &next_slot,
                                         &self.last_cs_final_periods, self.cfg.periods_per_cycle,
                                         self.cfg.denunciation_expire_cycle_delta)
        });
        self.block_header_by_slot.retain(|slot, _| {
            !is_expired_for_denunciation(slot, &next_slot,
                                         &self.last_cs_final_periods, self.cfg.periods_per_cycle,
                                         self.cfg.denunciation_expire_cycle_delta)
        });
        self.seen_endorsement_denunciation.retain(|(slot, _index)| {
            !is_expired_for_denunciation(slot, &next_slot,
                                         &self.last_cs_final_periods, self.cfg.periods_per_cycle,
                                         self.cfg.denunciation_expire_cycle_delta)
        });
        self.seen_block_header_denunciation.retain(|slot| {
            !is_expired_for_denunciation(slot, &next_slot,
                                         &self.last_cs_final_periods, self.cfg.periods_per_cycle,
                                         self.cfg.denunciation_expire_cycle_delta)
        });
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
                            self.process_new_ops(ops);
                        }
                        Ok(DenunciationInterest::WrappedHeader(wrapped_header)) => {
                            self.process_new_block_header(wrapped_header);
                        }
                        Ok(DenunciationInterest::Final(final_periods)) => {
                            self.last_cs_final_periods = final_periods;
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


/// Return true if denunciation slot is expired (either final or in tool old cycle)
fn is_expired_for_denunciation(denunciation_slot: &Slot, next_slot: &Slot,
                               last_cs_final_periods: &[u64], periods_per_cycle: u64,
                               denunciation_expire_cycle_delta: u64) -> bool
{
    // Slot is final => cannot be Denounced anymore
    if denunciation_slot.period <= last_cs_final_periods[denunciation_slot.thread as usize] {
        return true;
    }

    // As we need to ensure that the Denounced has Deferred credit
    // we reject Denunciation older than 3 cycle compared to the next slot
    let cycle = denunciation_slot.get_cycle(periods_per_cycle);
    let next_cycle = next_slot.get_cycle(periods_per_cycle);

    if (next_cycle - cycle) > denunciation_expire_cycle_delta {
        return true;
    }

    false
}
