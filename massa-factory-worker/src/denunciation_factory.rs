use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::thread;

use crossbeam_channel::{select, Receiver};
use tracing::{debug, info, warn};

use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::address::Address;
use massa_models::block_header::SecuredHeader;
use massa_models::denunciation::{
    BlockHeaderDenunciationInterest, Denunciation, DenunciationInterest,
    EndorsementDenunciationInterest,
};
use massa_models::endorsement::SecureShareEndorsement;
use massa_models::slot::Slot;
use massa_models::timeslots::get_closest_slot_to_timestamp;
use massa_time::MassaTime;

// TODO: rework these values
const DENUNCIATION_FACTORY_ENDORSEMENT_CACHE_MAX_LEN: usize = 4096;
const DENUNCIATION_FACTORY_BLOCK_HEADER_CACHE_MAX_LEN: usize = 4096;

/// Structure gathering all elements needed by the factory thread
pub(crate) struct DenunciationFactoryWorker {
    cfg: FactoryConfig,
    channels: FactoryChannels,
    /// Factory manager command receiver
    factory_receiver: Receiver<()>,
    consensus_receiver: Receiver<DenunciationInterest>,
    endorsement_pool_receiver: Receiver<DenunciationInterest>,
    /// Internal cache for endorsement denunciation
    /// store at most 1 endorsement per entry, as soon as we have 2 we produce a Denunciation
    endorsements_by_slot_index: HashMap<(Slot, u32), EndorsementDenunciationStatus>,
    /// Internal cache for block header denunciation
    block_header_by_slot: HashMap<Slot, BlockHeaderDenunciationStatus>,
}

impl DenunciationFactoryWorker {
    /// Creates the `FactoryThread` structure to gather all data and references
    /// needed by the factory worker thread.
    pub(crate) fn spawn(
        cfg: FactoryConfig,
        channels: FactoryChannels,
        factory_receiver: Receiver<()>,
        consensus_receiver: Receiver<DenunciationInterest>,
        endorsement_pool_receiver: Receiver<DenunciationInterest>,
    ) -> thread::JoinHandle<()> {
        thread::Builder::new()
            .name("denunciation-factory".into())
            .spawn(|| {
                let mut factory = Self {
                    cfg,
                    channels,
                    factory_receiver,
                    consensus_receiver,
                    endorsement_pool_receiver,
                    endorsements_by_slot_index: Default::default(),
                    block_header_by_slot: Default::default(),
                };
                factory.run();
            })
            .expect("failed to spawn thread : denunciation-factory")
    }

    /// Process new secured header (~ block header)
    fn process_new_secured_header(
        &mut self,
        block_header_denunciation_interest: DenunciationInterest,
    ) {
        let de_i_orig = block_header_denunciation_interest.clone();
        let de_i = match block_header_denunciation_interest {
            DenunciationInterest::Endorsement(_) => {
                return;
            }
            DenunciationInterest::BlockHeader(de_i) => de_i,
        };

        let now = MassaTime::now().expect("could not get current time");

        // get closest slot according to the current absolute time
        let slot_now = get_closest_slot_to_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            now,
        );

        let cycle_of_header = de_i.slot.get_cycle(self.cfg.periods_per_cycle);
        let cycle_now = slot_now.get_cycle(self.cfg.periods_per_cycle);

        // Do not fulfill the cache with block header too much in the future
        // Note that cache is also purged each time a slot becomes final
        if cycle_now - cycle_of_header > self.cfg.denunciation_items_max_cycle_delta {
            warn!(
                "Denunciation factory received a denunciation interest way to much in the future: {:?}",
                de_i
            );
            return;
        }

        // Get selected address from selector and check
        // Note: If the public key of the header creator is not checked to match the PoS,
        //       someone can spam with headers coming from various non-PoS-drawn pubkeys
        //       and cause a problem
        let selected_address = self.channels.selector.get_producer(de_i.slot);
        match selected_address {
            Ok(address) => {
                if address != Address::from_public_key(&de_i.public_key) {
                    warn!("Denunciation factory received a secured header but address was not selected");
                    return;
                }
            }
            Err(e) => {
                warn!("Cannot get producer from selector: {}", e);
            }
        }

        let denunciation_: Option<Denunciation> = match self.block_header_by_slot.entry(de_i.slot) {
            Entry::Occupied(mut eo) => {
                match eo.get_mut() {
                    BlockHeaderDenunciationStatus::Accumulating(de_i_1_) => {
                        let de_i_1: &DenunciationInterest = de_i_1_;
                        match Denunciation::try_from((de_i_1, &de_i_orig.clone())) {
                            Ok(de) => {
                                eo.insert(BlockHeaderDenunciationStatus::DenunciationEmitted);
                                Some(de)
                            }
                            Err(e) => {
                                debug!("Denunciation factory cannot create denunciation from endorsements: {}", e);
                                None
                            }
                        }
                    }
                    BlockHeaderDenunciationStatus::DenunciationEmitted => {
                        // Already 2 entries - so a Denunciation has already been created
                        None
                    }
                }
            }
            Entry::Vacant(ev) => {
                ev.insert(BlockHeaderDenunciationStatus::Accumulating(de_i_orig));
                None
            }
        };

        if let Some(denunciation) = denunciation_ {
            info!(
                "Created a new block header denunciation : {:?}",
                denunciation
            );

            self.channels.pool.add_denunciation(denunciation);
        }

        self.cleanup_cache();
    }

    /// Process new secure share endorsement (~ endorsement)
    fn process_new_secure_share_endorsement(
        &mut self,
        endorsement_denunciation_interest: DenunciationInterest,
    ) {
        let de_i_orig = endorsement_denunciation_interest.clone();
        let de_i = match endorsement_denunciation_interest {
            DenunciationInterest::Endorsement(de_i) => de_i,
            DenunciationInterest::BlockHeader(de_i) => {
                return;
            }
        };

        let now = MassaTime::now().expect("could not get current time");

        // get closest slot according to the current absolute time
        let slot_now = get_closest_slot_to_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            now,
        );

        let cycle_of_header = de_i.slot.get_cycle(self.cfg.periods_per_cycle);
        let cycle_now = slot_now.get_cycle(self.cfg.periods_per_cycle);

        // Do not fulfill the cache with block header too much in the future
        // Note that cache is also purged each time a slot becomes final
        if cycle_now - cycle_of_header > self.cfg.denunciation_items_max_cycle_delta {
            warn!(
                "Denunciation factory received a denunciation interest way to much in the future: {:?}",
                de_i
            );
            return;
        }

        // Get selected address from selector and check
        let selected = self.channels.selector.get_selection(de_i.slot);
        match selected {
            Ok(selection) => {
                if let Some(address) = selection.endorsements.get(de_i.index as usize) {
                    if *address != Address::from_public_key(&de_i.public_key) {
                        warn!("Denunciation factory received a secure share endorsement but address was not selected");
                        return;
                    }
                } else {
                    warn!("Denunciation factory could not get selected address for endorsements at index");
                    return;
                }
            }
            Err(e) => {
                warn!("Cannot get producer from selector: {}", e);
            }
        }

        if self.endorsements_by_slot_index.len() > DENUNCIATION_FACTORY_ENDORSEMENT_CACHE_MAX_LEN {
            warn!(
                "Denunciation factory cannot process - cache full: {}",
                self.endorsements_by_slot_index.len()
            );
            return;
        }

        let denunciation_: Option<Denunciation> = match self
            .endorsements_by_slot_index
            .entry((de_i.slot, de_i.index))
        {
            Entry::Occupied(mut eo) => {
                match eo.get_mut() {
                    EndorsementDenunciationStatus::Accumulating(de_i_1_) => {
                        let de_i_1: &DenunciationInterest = de_i_1_;
                        match Denunciation::try_from((de_i_1, &de_i_orig.clone())) {
                            Ok(de) => {
                                eo.insert(EndorsementDenunciationStatus::DenunciationEmitted);
                                Some(de)
                            }
                            Err(e) => {
                                debug!("Denunciation factory cannot create denunciation from endorsements: {}", e);
                                None
                            }
                        }
                    }
                    EndorsementDenunciationStatus::DenunciationEmitted => {
                        // A Denunciation has already been created, nothing to do here
                        None
                    }
                }
            }
            Entry::Vacant(ev) => {
                ev.insert(EndorsementDenunciationStatus::Accumulating(de_i_orig));
                None
            }
        };

        if let Some(denunciation) = denunciation_ {
            info!(
                "Created a new endorsement denunciation : {:?}",
                denunciation
            );
            self.channels.pool.add_denunciation(denunciation);
        }

        self.cleanup_cache();
    }

    fn cleanup_cache(&mut self) {
        self.endorsements_by_slot_index.retain(|(slot, _index), _| {
            !Denunciation::is_expired(
                slot,
                self.channels.pool.get_final_cs_periods(),
                self.cfg.periods_per_cycle,
                self.cfg.denunciation_expire_cycle_delta,
            )
        });

        self.block_header_by_slot.retain(|slot, _| {
            !Denunciation::is_expired(
                slot,
                self.channels.pool.get_final_cs_periods(),
                self.cfg.periods_per_cycle,
                self.cfg.denunciation_expire_cycle_delta,
            )
        });
    }

    /// main run loop of the denunciation creator thread
    fn run(&mut self) {
        loop {
            select! {
                recv(self.consensus_receiver) -> de_i_ => {
                    match de_i_ {
                        Ok(de_i) => {
                            info!("Denunciation factory receives a new block header denunciation interest: {:?}", de_i);
                            self.process_new_secured_header(de_i);
                        },
                        Err(e) => {
                            warn!("Denunciation factory cannot receive from consensus receiver: {}", e);
                            break;
                        }
                    }
                },
                recv(self.endorsement_pool_receiver) -> de_i_ => {
                    match de_i_ {
                        Ok(de_i) => {
                            info!("Denunciation factory receives a new endorsement denunciation interest: {:?}", de_i);
                            self.process_new_secure_share_endorsement(de_i)
                        }
                        Err(e) => {
                            warn!("Denunciation factory cannot receive from endorsement pool receiver: {}", e);
                            break;
                        }
                    }
                }
                recv(self.factory_receiver) -> msg_ => {
                    if let Err(e) = msg_ {
                        warn!("Denunciation factory receive an error from factory receiver: {}", e);
                    }
                    // factory_receiver send () and is supposed to be a STOP message
                    break;
                }
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum EndorsementDenunciationStatus {
    Accumulating(DenunciationInterest),
    DenunciationEmitted,
}

#[allow(clippy::large_enum_variant)]
enum BlockHeaderDenunciationStatus {
    Accumulating(DenunciationInterest),
    DenunciationEmitted,
}
