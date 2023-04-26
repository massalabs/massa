//! Copyright (c) 2023 MASSA LABS <info@massa.net>

use massa_execution_exports::ExecutionController;
use std::collections::{btree_map::Entry, BTreeMap};
use tracing::{debug, info, warn};

use massa_models::denunciation::DenunciationIndex;
use massa_models::slot::Slot;
use massa_models::{
    address::Address,
    denunciation::{Denunciation, DenunciationPrecursor},
    timeslots::get_closest_slot_to_timestamp,
};
use massa_pool_exports::PoolConfig;
use massa_pos_exports::SelectorController;
use massa_storage::Storage;
use massa_time::MassaTime;

pub struct DenunciationPool {
    /// pool configuration
    config: PoolConfig,
    /// selector controller to get draws
    pub selector: Box<dyn SelectorController>,
    /// execution controller
    execution_controller: Box<dyn ExecutionController>,
    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
    /// Internal cache for denunciations
    denunciations_cache: BTreeMap<DenunciationIndex, DenunciationStatus>,
}

impl DenunciationPool {
    pub fn init(
        config: PoolConfig,
        selector: Box<dyn SelectorController>,
        execution_controller: Box<dyn ExecutionController>,
    ) -> Self {
        Self {
            config,
            selector,
            execution_controller,
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

    /// Checks whether an element is stored in the pool - only used in unit tests for now
    #[cfg(feature = "testing")]
    pub fn contains(&self, denunciation: &Denunciation) -> bool {
        self.denunciations_cache
            .iter()
            .find(|(_, de_st)| match *de_st {
                DenunciationStatus::Accumulating(_) => false,
                DenunciationStatus::DenunciationEmitted(de) => de == denunciation,
            })
            .is_some()
    }

    /// Add a denunciation precursor to the pool - can lead to a Denunciation creation
    /// Note that the Denunciation is stored in the denunciation pool internal cache
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

        if Denunciation::is_expired(
            &slot.period,
            &self.last_cs_final_periods[slot.thread as usize],
            &self.config.denunciation_expire_periods,
        ) {
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
                        return;
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
                        return;
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

    /// cleanup internal cache, removing too old denunciation
    fn cleanup_caches(&mut self) {
        self.denunciations_cache.retain(|de_idx, _| {
            let slot = de_idx.get_slot();
            !Denunciation::is_expired(
                &slot.period,
                &self.last_cs_final_periods[slot.thread as usize],
                &self.config.denunciation_expire_periods,
            )
        });
    }

    /// get denunciations for block creation
    pub fn get_block_denunciations(&self, target_slot: &Slot) -> Vec<Denunciation> {
        let mut res = Vec::with_capacity(self.config.max_denunciations_per_block_header as usize);
        for (de_idx, de_status) in &self.denunciations_cache {
            if let DenunciationStatus::DenunciationEmitted(de) = de_status {
                // Checks
                // 1. the denunciation has not been executed already
                // 2. Denounced item slot is equal or before target slot of block header
                // 3. Denounced item slot is not too old
                let de_slot = de.get_slot();
                if !self.execution_controller.is_denunciation_executed(de_idx)
                    && de_slot <= target_slot
                    && !Denunciation::is_expired(
                        &de_slot.period,
                        &target_slot.period,
                        &self.config.denunciation_expire_periods,
                    )
                {
                    res.push(de.clone());
                }
            }

            if res.len() >= self.config.max_denunciations_per_block_header as usize {
                break;
            }
        }
        res
    }

    /// Notify of final periods
    pub(crate) fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final CS period counter
        self.last_cs_final_periods = final_cs_periods.to_vec();

        // remove all denunciations that are expired
        self.cleanup_caches()
    }

    /// Add endorsements, turn them in DenunciationPrecursor then process
    pub(crate) fn add_endorsements(&mut self, endorsement_storage: Storage) {
        let items = endorsement_storage
            .get_endorsement_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        let endo_store = endorsement_storage.read_endorsements();

        for endo_id in items {
            let endo = endo_store.get(&endo_id).expect(
                "Attempting to add endorsement to denunciation pool, but it is absent from storage",
            );

            if let Ok(de_p) = DenunciationPrecursor::try_from(endo) {
                self.add_denunciation_precursor(de_p)
            }
        }
    }
}

/// A Value (as in Key/Value) for denunciation pool internal cache
#[derive(Debug)]
enum DenunciationStatus {
    /// Only 1 DenunciationPrecursor received for this key
    Accumulating(DenunciationPrecursor),
    /// 2 DenunciationPrecursor received, a Denunciation was then created
    DenunciationEmitted(Denunciation),
}
