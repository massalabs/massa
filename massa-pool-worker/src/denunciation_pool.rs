use std::collections::{hash_map::Entry, HashMap};
use tracing::{debug, info, warn};

use massa_models::slot::Slot;
use massa_models::{
    address::Address,
    denunciation::{
        BlockHeaderDenunciationPrecursor, Denunciation, DenunciationPrecursor,
        EndorsementDenunciationPrecursor,
    },
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
    // /// internal cache
    // denunciations_cache: PreHashMap<DenunciationIndex, Denunciation>,
    /// Internal cache for endorsement denunciation
    /// store at most 1 endorsement per entry, as soon as we have 2 we produce a Denunciation
    endorsements_by_slot_index: HashMap<(Slot, u32), EndorsementDenunciationStatus>,
    /// Internal cache for block header denunciation
    block_header_by_slot: HashMap<Slot, BlockHeaderDenunciationStatus>,
}

impl DenunciationPool {
    pub fn init(config: PoolConfig, selector: Box<dyn SelectorController>) -> Self {
        Self {
            config,
            selector,
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            // denunciations_cache: Default::default(),
            endorsements_by_slot_index: Default::default(),
            block_header_by_slot: Default::default(),
        }
    }

    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        // self.denunciations_cache.len()
        todo!()
    }

    // Not used yet
    /*
    /// Checks whether an element is stored in the pool
    pub fn contains(&self, id: &DenunciationId) -> bool {
        self.storage.get_denunciation_refs().contains(id)
    }
    */

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
        let selected_address = self.selector.get_producer(*slot);
        match selected_address {
            Ok(address) => {
                if address != Address::from_public_key(denunciation_precursor.get_public_key()) {
                    warn!("Denunciation factory received a secured header but address was not selected");
                    return;
                }
            }
            Err(e) => {
                warn!("Cannot get producer from selector: {}", e);
            }
        }

        let denunciation_: Option<Denunciation> = match denunciation_precursor {
            DenunciationPrecursor::Endorsement(de_p) => {
                self.add_endorsement_denunciation_precursor(de_p)
            }
            DenunciationPrecursor::BlockHeader(de_p) => {
                self.add_block_header_denunciation_precursor(de_p)
            }
        };

        if let Some(denunciation) = denunciation_ {
            info!("Created a new denunciation : {:?}", denunciation);
        }

        self.cleanup_caches();
    }

    fn add_endorsement_denunciation_precursor(
        &mut self,
        denunciation_precursor: EndorsementDenunciationPrecursor,
    ) -> Option<Denunciation> {
        let key = (denunciation_precursor.slot, denunciation_precursor.index);
        match self.endorsements_by_slot_index.entry(key) {
            Entry::Occupied(mut eo) => match eo.get_mut() {
                EndorsementDenunciationStatus::Accumulating(de_p_1_) => {
                    let de_p_1: &DenunciationPrecursor = de_p_1_;
                    let de_p_2 = DenunciationPrecursor::Endorsement(denunciation_precursor);
                    match Denunciation::try_from((de_p_1, &de_p_2)) {
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
                    // Already 2 entries - so a Denunciation has already been created
                    None
                }
            },
            Entry::Vacant(ev) => {
                ev.insert(EndorsementDenunciationStatus::Accumulating(
                    DenunciationPrecursor::Endorsement(denunciation_precursor),
                ));
                None
            }
        }
    }

    fn add_block_header_denunciation_precursor(
        &mut self,
        denunciation_precursor: BlockHeaderDenunciationPrecursor,
    ) -> Option<Denunciation> {
        match self.block_header_by_slot.entry(denunciation_precursor.slot) {
            Entry::Occupied(mut eo) => match eo.get_mut() {
                BlockHeaderDenunciationStatus::Accumulating(de_p_1_) => {
                    let de_p_1: &DenunciationPrecursor = de_p_1_;
                    let de_p_2 = DenunciationPrecursor::BlockHeader(denunciation_precursor);
                    match Denunciation::try_from((de_p_1, &de_p_2)) {
                        Ok(de) => {
                            eo.insert(BlockHeaderDenunciationStatus::DenunciationEmitted);
                            Some(de)
                        }
                        Err(e) => {
                            debug!("Denunciation factory cannot create denunciation from block headers: {}", e);
                            None
                        }
                    }
                }
                BlockHeaderDenunciationStatus::DenunciationEmitted => {
                    // Already 2 entries - so a Denunciation has already been created
                    None
                }
            },
            Entry::Vacant(ev) => {
                ev.insert(BlockHeaderDenunciationStatus::Accumulating(
                    DenunciationPrecursor::BlockHeader(denunciation_precursor),
                ));
                None
            }
        }
    }

    fn cleanup_caches(&mut self) {
        self.endorsements_by_slot_index.retain(|(slot, _index), _| {
            !Denunciation::is_expired(
                slot,
                &self.last_cs_final_periods,
                self.config.denunciation_expire_periods,
            )
        });

        self.block_header_by_slot.retain(|slot, _| {
            !Denunciation::is_expired(
                slot,
                &self.last_cs_final_periods,
                self.config.denunciation_expire_periods,
            )
        });
    }

    /*
    /// Add a list of denunciation to the pool
    pub(crate) fn add_denunciation(&mut self, denunciation: Denunciation) {
        if !Denunciation::is_expired(
            denunciation.get_slot(),
            &self.last_cs_final_periods,
            self.config.denunciation_expire_periods,
        ) {
            let de_idx = DenunciationIndex::from(&denunciation);
            self.denunciations_cache.insert(de_idx, denunciation);
        }
    }
    */

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
