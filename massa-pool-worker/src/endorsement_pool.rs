//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block_id::BlockId,
    endorsement::EndorsementId,
    prehash::{CapacityAllocator, PreHashSet},
    slot::Slot,
};
use massa_pool_exports::PoolConfig;
use massa_storage::Storage;
use std::collections::{BTreeMap, HashMap};

pub struct EndorsementPool {
    /// configuration
    config: PoolConfig,

    /// endorsements indexed by slot, index and block ID
    endorsements_indexed: HashMap<(Slot, u32, BlockId), EndorsementId>,

    /// endorsements sorted by increasing inclusion slot for pruning
    /// indexed by thread, then `BTreeMap<(inclusion_slot, index, target_block), endorsement_id>`
    endorsements_sorted: Vec<BTreeMap<(Slot, u32, BlockId), EndorsementId>>,

    /// storage
    storage: Storage,

    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,
}

impl EndorsementPool {
    pub fn init(config: PoolConfig, storage: &Storage) -> Self {
        EndorsementPool {
            last_cs_final_periods: vec![0u64; config.thread_count.get() as usize],
            endorsements_indexed: Default::default(),
            endorsements_sorted: vec![Default::default(); config.thread_count.get() as usize],
            config,
            storage: storage.clone_without_refs(),
        }
    }

    /// Get the number of stored elements
    pub fn len(&self) -> usize {
        self.storage.get_endorsement_refs().len()
    }

    /// Checks whether an element is stored in the pool.
    pub fn contains(&self, id: &EndorsementId) -> bool {
        self.storage.get_endorsement_refs().contains(id)
    }

    /// notify of new final CS periods
    pub(crate) fn notify_final_cs_periods(&mut self, final_cs_periods: &[u64]) {
        // update internal final CS period counter
        self.last_cs_final_periods = final_cs_periods.to_vec();

        // remove all endorsements whose periods <= last_cs_final_periods[endorsement.thread]
        let mut removed: PreHashSet<EndorsementId> = Default::default();
        for thread in 0..self.config.thread_count.get() {
            while let Some((&(inclusion_slot, index, block_id), &endo_id)) =
                self.endorsements_sorted[thread as usize].first_key_value()
            {
                if inclusion_slot.period <= self.last_cs_final_periods[thread as usize] {
                    self.endorsements_sorted[thread as usize].pop_first();
                    self.endorsements_indexed
                        .remove(&(inclusion_slot, index, block_id))
                        .expect("endorsement should be in endorsements_indexed at this point");
                    removed.insert(endo_id);
                } else {
                    break;
                }
            }
        }
        self.storage.drop_endorsement_refs(&removed);
    }

    /// Add a list of endorsements to the pool
    pub(crate) fn add_endorsements(&mut self, mut endorsement_storage: Storage) {
        let items = endorsement_storage
            .get_endorsement_refs()
            .iter()
            .copied()
            .collect::<Vec<_>>();

        let mut added = PreHashSet::with_capacity(items.len());
        let mut removed = PreHashSet::with_capacity(items.len());

        // add items to pool
        {
            let endo_store = endorsement_storage.read_endorsements();
            for endo_id in items {
                let endo = endo_store
                    .get(&endo_id)
                    .expect("attempting to add endorsement to pool, but it is absent from storage");

                if endo.content.slot.period
                    < self.last_cs_final_periods[endo.content.slot.thread as usize]
                {
                    // endorsement expired: ignore
                    continue;
                }

                // insert
                let key = (
                    endo.content.slot,
                    endo.content.index,
                    endo.content.endorsed_block,
                );
                // note that we don't want equivalent endorsements (slot, index, block etc...) to overwrite each other
                if self.endorsements_indexed.try_insert(key, endo.id).is_ok() {
                    if self.endorsements_sorted[endo.content.slot.thread as usize]
                        .insert(key, endo.id)
                        .is_some()
                    {
                        panic!("endorsement is expected to be absent from endorsements_sorted at this point");
                    }
                    added.insert(endo.id);
                }
            }
        }

        // prune excess endorsements
        for thread in 0..self.config.thread_count.get() {
            while self.endorsements_sorted[thread as usize].len()
                > self.config.max_endorsements_pool_size_per_thread
            {
                // won't panic because len was checked above
                let (_key, endo_id) = self.endorsements_sorted[thread as usize]
                    .pop_last()
                    .unwrap();
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
        slot: &Slot, // slot of the block that will contain the endorsement
        target_block: &BlockId,
    ) -> (Vec<Option<EndorsementId>>, Storage) {
        // init list of selected endorsement IDs
        let mut endo_ids = Vec::with_capacity(self.config.max_block_endorsement_count as usize);

        // gather endorsements
        for index in 0..self.config.max_block_endorsement_count {
            endo_ids.push(
                self.endorsements_indexed
                    .get(&(*slot, index, *target_block))
                    .copied(),
            );
        }

        // setup endorsement storage
        let mut endo_storage = self.storage.clone_without_refs();
        let claim_endos: PreHashSet<EndorsementId> =
            endo_ids.iter().filter_map(|&opt| opt).collect();
        let claimed_endos = endo_storage.claim_endorsement_refs(&claim_endos);
        if claimed_endos.len() != claim_endos.len() {
            panic!("could not claim all endorsements from storage");
        }

        (endo_ids, endo_storage)
    }
}
