//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    prehash::{BuildMap, Set},
    BlockId, EndorsementId, Slot,
};
use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use std::collections::{BTreeSet, HashMap};

pub struct EndorsementPool {
    /// config
    pub config: PoolConfig,

    /// endorsement hashmap indexed by target slot and target block ID for fast access (slot, index, block_id)
    pub endorsements: HashMap<(Slot, u32, BlockId), EndorsementId>,

    /// endorsements sorted by increasing target slot for pruning
    /// indexed by thread, then BTreeSet<(target_slot, index, target_block)>
    pub endorsements_sorted: Vec<BTreeSet<(Slot, u32, BlockId)>>,

    /// storage
    pub storage: Storage,

    /// last consensus final periods, per thread
    pub last_cs_final_periods: Vec<u64>,
}

impl EndorsementPool {
    pub fn init(config: PoolConfig, storage: Storage) -> Self {
        EndorsementPool {
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            endorsements: Default::default(),
            endorsements_sorted: Default::default(),
            config,
            storage,
        }
    }

    /// notify of new final CS periods
    pub fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final CS period counter
        self.last_cs_final_periods = final_cs_periods.to_vec();

        // remove old endorsements
        let mut removed: Set<EndorsementId> = Default::default();
        for thread in 0..self.config.thread_count {
            while let Some((target_slot, target_block, index)) =
                self.endorsements_sorted[thread as usize].first().copied()
            {
                if target_slot.period < self.last_cs_final_periods[thread as usize] {
                    self.endorsements_sorted[thread as usize].pop_first();
                    let e_id = self
                        .endorsements
                        .remove(&(target_slot, target_block, index))
                        .expect("endorsement pool index inconsistency");
                    removed.insert(e_id);
                }
            }
        }
        self.storage.drop_endorsement_refs(&removed);
    }

    /// Add a list of endorsements to the pool
    pub fn add_endorsements(&mut self, mut endorsement_storage: Storage) {
        let items = endorsement_storage
            .get_endorsement_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        let mut added = Set::with_capacity_and_hasher(items.len(), BuildMap::default());
        let mut removed = Set::with_capacity_and_hasher(items.len(), BuildMap::default());

        // add items to pool
        endorsement_storage.with_endorsements(&items, |endo_refs| {
            endo_refs
                .iter()
                .zip(items.iter())
                .for_each(|(endo_ref, _id)| {
                    let endo = endo_ref.expect(
                        "attempting to add endorsement to pool, but it is absent from storage",
                    );

                    if endo.content.slot.period
                        < self.last_cs_final_periods[endo.content.slot.thread as usize]
                    {
                        // endorsement expired: ignore
                        return;
                    }

                    let key = (
                        endo.content.slot,
                        endo.content.index,
                        endo.content.endorsed_block,
                    );
                    match self.endorsements.entry(key) {
                        std::collections::hash_map::Entry::Occupied(_) => {
                            // we already have an endorsement for this slot: ignore
                            return;
                        }
                        std::collections::hash_map::Entry::Vacant(vac) => {
                            vac.insert(endo.id);
                        }
                    }
                    self.endorsements_sorted[endo.content.slot.thread as usize].insert(key);
                    added.insert(endo.id);
                });
        });

        // prune excess endorsements
        for thread in 0..self.config.thread_count {
            while self.endorsements_sorted[thread as usize].len()
                > self.config.max_endorements_pool_size_per_thread
            {
                // won't panic because len was checked above
                let key = self.endorsements_sorted[thread as usize]
                    .pop_last()
                    .unwrap();
                let endo_id = self
                    .endorsements
                    .remove(&key)
                    .expect("endorsement pool index inconsistency on pos-insert pruning");
                if !added.remove(&endo_id) {
                    removed.insert(endo_id);
                }
            }
        }

        // take ownership on added endorsements
        self.storage.extend(endorsement_storage.split_off(
            &Default::default(),
            &Default::default(),
            &added,
        ));

        // drop removed endorsements from storage
        self.storage.drop_endorsement_refs(&removed);
    }

    /// get endorsements for block creation
    pub fn get_block_endorsements(
        &self,
        target_slot: &Slot,
        target_block: &BlockId,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        // init list of selected operation IDs
        let mut endo_ids = Vec::with_capacity(self.config.max_block_endorsement_count as usize);

        // gather endorsements
        for index in 0..self.config.max_block_endorsement_count {
            endo_ids.push(
                self.endorsements
                    .get(&(*target_slot, index, *target_block))
                    .copied(),
            );
        }

        // setup endorsement storage
        let mut endo_storage = self.storage.clone_without_refs();
        let claim_endos: Set<EndorsementId> = endo_ids.iter().filter_map(|&opt| opt).collect();
        let claimed_endos = endo_storage.claim_endorsement_refs(&claim_endos);
        if claimed_endos.len() != claim_endos.len() {
            panic!("could not claim all endorsements from storage");
        }

        (endo_ids, endo_storage)
    }
}
