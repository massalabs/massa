use massa_models::denunciation::{Denunciation, DenunciationId};
use massa_models::prehash::PreHashMap;

use massa_pool_exports::PoolConfig;

pub struct DenunciationPool {
    /// configuration
    config: PoolConfig,
    /// storage
    // storage: Storage,
    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
    /// internal cache
    denunciations_cache: PreHashMap<DenunciationId, Denunciation>,
}

impl DenunciationPool {
    pub fn init(config: PoolConfig) -> Self {
        Self {
            config,
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            denunciations_cache: Default::default(),
        }
    }

    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        self.denunciations_cache.len()
    }

    // Not used yet
    /*
    /// Checks whether an element is stored in the pool
    pub fn contains(&self, id: &DenunciationId) -> bool {
        self.storage.get_denunciation_refs().contains(id)
    }
    */

    /// Add a list of denunciation to the pool
    pub(crate) fn add_denunciation(&mut self, denunciation: Denunciation) {
        if !Denunciation::is_expired(
            denunciation.get_slot(),
            &self.last_cs_final_periods,
            self.config.denunciation_expire_periods,
        ) {
            let de_id = DenunciationId::from(&denunciation);
            self.denunciations_cache.insert(de_id, denunciation);
        }
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
        self.denunciations_cache.drain_filter(|_de_id, de| {
            Denunciation::is_expired(
                de.get_slot(),
                &self.last_cs_final_periods,
                self.config.denunciation_expire_periods,
            )
        });
    }
}
