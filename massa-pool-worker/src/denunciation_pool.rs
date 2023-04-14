use std::collections::{hash_map::Entry, HashMap};
use tracing::{debug, info, warn};

use massa_models::denunciation::DenunciationIndex;
use massa_models::{
    address::Address,
    denunciation::{Denunciation, DenunciationPrecursor},
    timeslots::get_closest_slot_to_timestamp,
};
use massa_pool_exports::PoolConfig;
use massa_pos_exports::SelectorController;
use massa_time::MassaTime;

pub struct DenunciationPool {
    /// configuration
    config: PoolConfig,
    /// selector controller to get draws
    pub selector: Box<dyn SelectorController>,
    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
    /// Internal cache for denunciations
    denunciations_cache: HashMap<DenunciationIndex, DenunciationStatus>,
}

impl DenunciationPool {
    pub fn init(config: PoolConfig, selector: Box<dyn SelectorController>) -> Self {
        Self {
            config,
            selector,
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            denunciations_cache: Default::default(),
        }
    }

    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        self.denunciations_cache
            .iter()
            .filter(|(_, de_st)| matches!(*de_st, DenunciationStatus::DenunciationEmitted(..)))
            .count()
    }

    /// Checks whether an element is stored in the pool
    pub fn contains(&self, denunciation: &Denunciation) -> bool {
        self.denunciations_cache
            .iter()
            .find(|(_, de_st)| match *de_st {
                DenunciationStatus::Accumulating(_) => false,
                DenunciationStatus::DenunciationEmitted(de) => de == denunciation,
            })
            .is_some()
    }

    pub fn add_denunciation_precursor(&mut self, denunciation_precursor: DenunciationPrecursor) {
        let slot = denunciation_precursor.get_slot();

        // Do some checkups before adding the denunciation precursor

        let now = MassaTime::now().expect("could not get current time");

        // get closest slot according to the current absolute time
        let slot_now = get_closest_slot_to_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now,
        );

        // let last_final_periods = self.channels.pool.get_final_cs_periods();
        if slot.period
            < self.last_cs_final_periods[slot.thread as usize]
                .saturating_sub(self.config.denunciation_expire_periods)
        {
            // too old - cannot be denounced anymore
            return;
        }

        if slot.period.saturating_sub(slot_now.period) > self.config.denunciation_expire_periods {
            // too much in the future - ignored
            return;
        }

        // Get selected address from selector and check
        // Note: If the public key of the header creator is not checked to match the PoS,
        //       someone can spam with headers coming from various non-PoS-drawn pubkeys
        //       and cause a problem
        match &denunciation_precursor {
            DenunciationPrecursor::Endorsement(de_p) => {
                // Get selected address from selector and check
                let selected = self.selector.get_selection(de_p.slot);
                match selected {
                    Ok(selection) => {
                        if let Some(address) = selection.endorsements.get(de_p.index as usize) {
                            if *address != Address::from_public_key(&de_p.public_key) {
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
            }
            DenunciationPrecursor::BlockHeader(de_p) => {
                let selected_address = self.selector.get_producer(de_p.slot);
                match selected_address {
                    Ok(address) => {
                        if address
                            != Address::from_public_key(denunciation_precursor.get_public_key())
                        {
                            warn!("Denunciation factory received a secured header but address was not selected");
                            return;
                        }
                    }
                    Err(e) => {
                        warn!("Cannot get producer from selector: {}", e);
                    }
                }
            }
        }

        let key = DenunciationIndex::from(&denunciation_precursor);

        let denunciation_: Option<Denunciation> = match self.denunciations_cache.entry(key) {
            Entry::Occupied(mut eo) => match eo.get_mut() {
                DenunciationStatus::Accumulating(de_p_) => {
                    let de_p: &DenunciationPrecursor = de_p_;
                    match Denunciation::try_from((de_p, &denunciation_precursor)) {
                        Ok(de) => {
                            eo.insert(DenunciationStatus::DenunciationEmitted(de.clone()));
                            Some(de)
                        }
                        Err(e) => {
                            debug!("Denunciation factory cannot create denunciation from endorsements: {}", e);
                            None
                        }
                    }
                }
                DenunciationStatus::DenunciationEmitted(..) => {
                    // Already 2 entries - so a Denunciation has already been created
                    None
                }
            },
            Entry::Vacant(ev) => {
                ev.insert(DenunciationStatus::Accumulating(denunciation_precursor));
                None
            }
        };

        if let Some(denunciation) = denunciation_ {
            info!("Created a new denunciation : {:?}", denunciation);
        }

        self.cleanup_caches();
    }

    fn cleanup_caches(&mut self) {
        self.denunciations_cache.retain(|de_idx, _| {
            !Denunciation::is_expired(
                de_idx.get_slot(),
                &self.last_cs_final_periods,
                self.config.denunciation_expire_periods,
            )
        });
    }

    // In next PR
    /*
    pub fn get_block_denunciations(
        &self,
        slot: &Slot,
        target_block: &BlockId,
    ) -> Vec<DenunciationId> {
        todo!()
    }
    */

    pub(crate) fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final CS period counter
        self.last_cs_final_periods = final_cs_periods.to_vec();

        // remove all denunciations that are expired
        self.cleanup_caches()
    }
}

enum DenunciationStatus {
    Accumulating(DenunciationPrecursor),
    DenunciationEmitted(Denunciation),
}
