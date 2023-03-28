use massa_models::prehash::{CapacityAllocator, PreHashSet};

use massa_pool_exports::PoolConfig;
use massa_storage::Storage;

pub struct DenunciationPool {
    /// configuration
    config: PoolConfig,
    /// storage
    storage: Storage,
}

impl DenunciationPool {
    pub fn init(config: PoolConfig, storage: &Storage) -> Self {
        Self {
            config,
            storage: storage.clone_without_refs(),
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
        let mut removed = PreHashSet::with_capacity(denunciation_ids.len());

        // populate added
        {
            let de_indexes = denunciation_storage.read_denunciations();
            for de_id in denunciation_ids {
                let _de = de_indexes.get(&de_id).expect(
                    "Attempting to add denunciation to pool but it is absent from given storage",
                );

                // TODO: do not add denunciation if expired

                // TODO: check that we do not add "duplicate"
                added.insert(de_id);
            }
        }

        // populate removed
        // TODO: remove from self storage

        // take ownership on added denunciations
        self.storage.extend(denunciation_storage.split_off(
            &Default::default(),
            &Default::default(),
            &Default::default(),
            &added,
        ));

        // drop removed endorsements from storage
        self.storage.drop_denunciation_refs(&removed);
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
}
