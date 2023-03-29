use massa_models::denunciation::{Denunciation, DenunciationId};
use massa_models::prehash::{CapacityAllocator, PreHashMap, PreHashSet};
use massa_models::slot::Slot;
use massa_models::timeslots::get_closest_slot_to_timestamp;

use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use massa_time::MassaTime;

pub struct DenunciationPool {
    /// configuration
    config: PoolConfig,
    /// storage
    storage: Storage,
    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
    /// internal cache
    denunciations_cache: PreHashMap<DenunciationId, Slot>,
}

impl DenunciationPool {
    pub fn init(config: PoolConfig, storage: &Storage) -> Self {
        Self {
            config,
            storage: storage.clone_without_refs(),
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            denunciations_cache: Default::default(),
        }
    }

    // Not used yet
    /*
    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        self.storage.get_denunciation_refs().len()
    }

    /// Checks whether an element is stored in the pool
    pub fn contains(&self, id: &DenunciationId) -> bool {
        self.storage.get_denunciation_refs().contains(id)
    }
    */

    /// Add a list of denunciation to the pool
    pub(crate) fn add_denunciation(&mut self, mut denunciation_storage: Storage) {
        let denunciation_ids = denunciation_storage
            .get_denunciation_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        let mut added = PreHashSet::with_capacity(denunciation_ids.len());

        // populate added
        {
            let de_indexes = denunciation_storage.read_denunciations();
            for de_id in denunciation_ids {
                let de = de_indexes.get(&de_id).expect(
                    "Attempting to add denunciation to pool but it is absent from given storage",
                );

                let now = MassaTime::now().expect("could not get current time");
                // get closest slot according to the current absolute time
                let slot_now = get_closest_slot_to_timestamp(
                    self.config.thread_count,
                    self.config.t0,
                    self.config.genesis_timestamp,
                    now,
                );
                if Denunciation::is_expired(
                    de.get_slot(),
                    &slot_now,
                    &self.last_cs_final_periods,
                    self.config.periods_per_cycle,
                    self.config.denunciation_expire_cycle_delta,
                ) {
                    continue;
                }

                self.denunciations_cache
                    .insert(de_id, de.get_slot().clone());
                added.insert(de_id);
            }
        }

        // take ownership on added denunciations
        self.storage.extend(denunciation_storage.split_off(
            &Default::default(),
            &Default::default(),
            &Default::default(),
            &added,
        ));
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
        let mut removed: PreHashSet<DenunciationId> = Default::default();

        let now = MassaTime::now().expect("could not get current time");
        // get closest slot according to the current absolute time
        let slot_now = get_closest_slot_to_timestamp(
            self.config.thread_count,
            self.config.t0,
            self.config.genesis_timestamp,
            now,
        );

        for (de_id, de_slot) in self.denunciations_cache.iter() {
            if Denunciation::is_expired(
                de_slot,
                &slot_now,
                &self.last_cs_final_periods,
                self.config.periods_per_cycle,
                self.config.denunciation_expire_cycle_delta,
            ) {
                removed.insert(de_id.clone());
            }
        }

        // Remove from internal cache
        for de_id in removed.iter() {
            self.denunciations_cache.remove(de_id);
        }

        // Remove from storage
        self.storage.drop_denunciation_refs(&removed);
    }
}
