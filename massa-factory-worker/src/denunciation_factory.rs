use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::thread;

use crossbeam_channel::{select, Receiver};
use tracing::{debug, info, warn};

use massa_factory_exports::{FactoryChannels, FactoryConfig};
use massa_models::address::Address;
use massa_models::denunciation::{Denunciation, DenunciationPrecursor};
use massa_models::slot::Slot;
use massa_models::timeslots::get_closest_slot_to_timestamp;
use massa_time::MassaTime;

/// Structure gathering all elements needed by the factory thread
pub(crate) struct DenunciationFactoryWorker {
    cfg: FactoryConfig,
    channels: FactoryChannels,
    /// Factory manager command receiver
    factory_receiver: Receiver<()>,
    consensus_receiver: Receiver<DenunciationPrecursor>,
    endorsement_pool_receiver: Receiver<DenunciationPrecursor>,
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
        consensus_receiver: Receiver<DenunciationPrecursor>,
        endorsement_pool_receiver: Receiver<DenunciationPrecursor>,
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
        block_header_denunciation_interest: DenunciationPrecursor,
    ) {
        let de_i_orig = block_header_denunciation_interest.clone();
        let de_i = match block_header_denunciation_interest {
            DenunciationPrecursor::Endorsement(_) => {
                return;
            }
            DenunciationPrecursor::BlockHeader(de_i) => de_i,
        };

        let now = MassaTime::now().expect("could not get current time");

        // get closest slot according to the current absolute time
        let slot_now = get_closest_slot_to_timestamp(
            self.cfg.thread_count,
            self.cfg.t0,
            self.cfg.genesis_timestamp,
            now,
        );

        let last_final_periods = self.channels.pool.get_final_cs_periods();
        if de_i.slot.period
            < last_final_periods[de_i.slot.thread as usize]
                .saturating_sub(self.cfg.denunciation_expire_periods)
        {
            // too old - cannot be denounced anymore
            return;
        }

        if de_i.slot.period.saturating_sub(slot_now.period) > self.cfg.denunciation_expire_periods {
            // too much in the future - ignored
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
                        let de_i_1: &DenunciationPrecursor = de_i_1_;
                        match Denunciation::try_from((de_i_1, &de_i_orig)) {
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
        endorsement_denunciation_interest: DenunciationPrecursor,
    ) {
        let de_i_orig = endorsement_denunciation_interest.clone();
        let de_i = match endorsement_denunciation_interest {
            DenunciationPrecursor::Endorsement(de_i) => de_i,
            DenunciationPrecursor::BlockHeader(_) => {
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

        let last_final_periods = self.channels.pool.get_final_cs_periods();
        if de_i.slot.period
            < last_final_periods[de_i.slot.thread as usize]
                .saturating_sub(self.cfg.denunciation_expire_periods)
        {
            // too old - cannot be denounced anymore
            return;
        }

        if de_i.slot.period.saturating_sub(slot_now.period) > self.cfg.denunciation_expire_periods {
            // too much in the future - ignored
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

        let denunciation_: Option<Denunciation> = match self
            .endorsements_by_slot_index
            .entry((de_i.slot, de_i.index))
        {
            Entry::Occupied(mut eo) => {
                match eo.get_mut() {
                    EndorsementDenunciationStatus::Accumulating(de_i_1_) => {
                        let de_i_1: &DenunciationPrecursor = de_i_1_;
                        match Denunciation::try_from((de_i_1, &de_i_orig)) {
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
                self.cfg.denunciation_expire_periods,
            )
        });

        self.block_header_by_slot.retain(|slot, _| {
            !Denunciation::is_expired(
                slot,
                self.channels.pool.get_final_cs_periods(),
                self.cfg.denunciation_expire_periods,
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
                            debug!("Denunciation factory receives a new block header denunciation interest: {:?}", de_i);
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
                            debug!("Denunciation factory receives a new endorsement denunciation interest: {:?}", de_i);
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
                        debug!("Denunciation factory receive an error from factory receiver: {}", e);
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
    Accumulating(DenunciationPrecursor),
    DenunciationEmitted,
}

#[allow(clippy::large_enum_variant)]
enum BlockHeaderDenunciationStatus {
    Accumulating(DenunciationPrecursor),
    DenunciationEmitted,
}
