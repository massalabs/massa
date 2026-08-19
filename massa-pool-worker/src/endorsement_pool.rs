//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    block_id::BlockId,
    endorsement::EndorsementId,
    prehash::{CapacityAllocator, PreHashMap, PreHashSet},
    slot::Slot,
};
use massa_pool_exports::{PoolChannels, PoolConfig};
use massa_storage::Storage;
use massa_wallet::Wallet;
use parking_lot::RwLock;
use std::{
    collections::{hash_map::Entry, BTreeMap},
    sync::Arc,
};
use tracing::{trace, warn};

/// Maximum number of endorsements kept for a given `(slot, index)` pair, whatever the endorsed
/// block is. Only one endorsement per `(slot, index)` can ever end up in a block, and an honest
/// endorser signs exactly one. The endorsed block is not constrained at ingress, so without this
/// bound an equivocating drawn endorser could mint arbitrarily many valid endorsements differing
/// only by their endorsed block and grow the pool without limit.
const MAX_ENDORSEMENTS_PER_SLOT_INDEX: usize = 1;

#[derive(Clone)]
pub struct EndorsementPool {
    /// configuration
    config: PoolConfig,

    /// endorsements sorted by increasing inclusion slot for pruning,
    /// indexed by thread, then `BTreeMap<(inclusion_slot, index), map of endorsement_id by target block>`.
    /// This is the single source of truth of the pool contents: keeping a second index keyed on the
    /// target block would risk desynchronizing on pruning and leak entries.
    endorsements_sorted: Vec<BTreeMap<(Slot, u32), PreHashMap<BlockId, EndorsementId>>>,

    /// storage
    storage: Storage,

    /// last consensus final periods, per thread
    last_cs_final_periods: Vec<u64>,

    /// channels used by the pool worker
    channels: PoolChannels,

    /// staking wallet, to know which addresses we are using to stake
    wallet: Arc<RwLock<Wallet>>,
}

impl EndorsementPool {
    pub fn init(
        config: PoolConfig,
        storage: &Storage,
        channels: PoolChannels,
        wallet: Arc<RwLock<Wallet>>,
    ) -> Self {
        EndorsementPool {
            last_cs_final_periods: vec![0u64; config.thread_count as usize],
            endorsements_sorted: vec![Default::default(); config.thread_count as usize],
            config,
            storage: storage.clone_without_refs(),
            channels,
            wallet,
        }
    }

    /// Replace the current endorsement pool contents with the contents of another one.
    /// This is used for double buffering.
    pub(crate) fn replace_with(&mut self, other: &EndorsementPool) {
        self.last_cs_final_periods = other.last_cs_final_periods.clone();
        self.endorsements_sorted = other.endorsements_sorted.clone();
        self.storage.replace_with(&other.storage);
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
        for thread in 0..self.config.thread_count {
            while let Some((&(inclusion_slot, _index), _)) =
                self.endorsements_sorted[thread as usize].first_key_value()
            {
                if inclusion_slot.period > self.last_cs_final_periods[thread as usize] {
                    break;
                }
                // won't panic because first_key_value returned an entry above
                let (_key, endos) = self.endorsements_sorted[thread as usize]
                    .pop_first()
                    .unwrap();
                removed.extend(endos.into_values());
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

                // check endorsement expiry
                if endo.content.slot.period
                    <= self.last_cs_final_periods[endo.content.slot.thread as usize]
                {
                    continue;
                }

                // check PoS draw
                let pos_draws = match self.channels.selector.get_selection(endo.content.slot) {
                    Ok(draw) => draw,
                    Err(err) => {
                        warn!(
                            "error, failed to get PoS draw for endorsement with id {} at slot {}: {}",
                            endo.id.clone(), endo.content.slot, err
                        );
                        continue;
                    }
                };
                if !pos_draws
                    .endorsements
                    .get(endo.content.index as usize)
                    .map_or(false, |a| a == &endo.content_creator_address)
                {
                    warn!(
                        "error, endorsement with id {} at slot {} is not selected for PoS draw",
                        endo.id.clone(),
                        endo.content.slot
                    );
                    continue;
                }

                // Broadcast endorsement to active channel subscribers.
                if self.config.broadcast_enabled {
                    if let Err(err) = self
                        .channels
                        .broadcasts
                        .endorsement_sender
                        .send(endo.clone())
                    {
                        trace!(
                            "error, failed to broadcast endorsement {}: {}",
                            endo.id.clone(),
                            err
                        );
                    }
                }

                // Only keep endorsements that one of our addresses can include
                if !self.wallet.read().keys.contains_key(&pos_draws.producer) {
                    continue;
                }

                // insert
                let by_target_block = self.endorsements_sorted[endo.content.slot.thread as usize]
                    .entry((endo.content.slot, endo.content.index))
                    .or_default();
                let is_full = by_target_block.len() >= MAX_ENDORSEMENTS_PER_SLOT_INDEX;
                // note that we don't want equivalent endorsements (slot, index, block etc...) to overwrite each other
                if let Entry::Vacant(e) = by_target_block.entry(endo.content.endorsed_block) {
                    // refuse conflicting endorsements beyond the per-(slot, index) bound: the
                    // endorsed block is not constrained at ingress, so without it an equivocating
                    // drawn endorser could flood the pool
                    if is_full {
                        warn!(
                            "ignoring endorsement {} at slot {} index {}: too many conflicting endorsements for that draw",
                            endo.id.clone(),
                            endo.content.slot,
                            endo.content.index
                        );
                        continue;
                    }
                    e.insert(endo.id);
                    added.insert(endo.id);
                }
            }
        }

        // prune excess endorsements
        for thread in 0..self.config.thread_count {
            while self.endorsements_sorted[thread as usize].len()
                > self.config.max_endorsements_pool_size_per_thread
            {
                // won't panic because len was checked above
                let (_key, endos) = self.endorsements_sorted[thread as usize]
                    .pop_last()
                    .unwrap();
                for endo_id in endos.into_values() {
                    if !added.remove(&endo_id) {
                        removed.insert(endo_id);
                    }
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
        let thread_endorsements = self.endorsements_sorted.get(slot.thread as usize);
        for index in 0..self.config.max_block_endorsement_count {
            endo_ids.push(thread_endorsements.and_then(|endos| {
                endos
                    .get(&(*slot, index))
                    .and_then(|by_target_block| by_target_block.get(target_block))
                    .copied()
            }));
        }

        // setup endorsement storage
        let mut endo_storage = self.storage.clone_without_refs();
        let claim_endos: PreHashSet<EndorsementId> =
            endo_ids.iter().filter_map(|&opt| opt).collect();
        let claimed_endos = endo_storage.claim_endorsement_refs(&claim_endos);
        if claimed_endos.len() != claim_endos.len() {
            // The pool holds a storage reference for every endorsement it indexes, so this should
            // not happen. Drop the unclaimable ones instead of killing the pool worker: producing a
            // block with fewer endorsements is always better than not producing one at all.
            warn!(
                "could not claim all endorsements from storage for a block at slot {}",
                slot
            );
            for endo_id in endo_ids.iter_mut() {
                if endo_id.map_or(false, |id| !claimed_endos.contains(&id)) {
                    *endo_id = None;
                }
            }
        }

        (endo_ids, endo_storage)
    }
}
