// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{PoolError, PoolSettings};
use models::{
    Address, BlockId, Endorsement, EndorsementContent, EndorsementHashMap, EndorsementHashSet,
    EndorsementId, Slot,
};

pub struct EndorsementPool {
    endorsements: EndorsementHashMap<Endorsement>,
    latest_final_periods: Vec<u64>,
    current_slot: Option<Slot>,
    pool_settings: &'static PoolSettings,
}

impl EndorsementPool {
    pub fn new(pool_settings: &'static PoolSettings, thread_count: u8) -> EndorsementPool {
        EndorsementPool {
            endorsements: Default::default(),
            pool_settings,
            current_slot: None,
            latest_final_periods: vec![0; thread_count as usize],
        }
    }

    pub fn len(&self) -> usize {
        self.endorsements.len()
    }

    /// Removes endorsements that are too old
    pub fn update_latest_final_periods(&mut self, periods: Vec<u64>) {
        self.endorsements.retain(
            |_,
             Endorsement {
                 content: EndorsementContent { slot, .. },
                 ..
             }| slot.period >= periods[slot.thread as usize],
        );
        self.latest_final_periods = periods;
    }

    /// gets ok endorsements for a given slot, with given endorsed block and endorsement creators at index
    /// returns sorted and dedupped endorsements
    pub fn get_endorsements(
        &self,
        target_slot: Slot,
        parent: BlockId,
        creators: Vec<Address>,
    ) -> Result<Vec<(EndorsementId, Endorsement)>, PoolError> {
        let mut candidates = self
            .endorsements
            .iter()
            .filter_map(|(endo_id, endorsement)| {
                let creator = match Address::from_public_key(&endorsement.content.sender_public_key)
                {
                    Ok(addr) => addr,
                    Err(e) => return Some(Err(e.into())),
                };
                if endorsement.content.endorsed_block == parent
                    && endorsement.content.slot == target_slot
                    && creators.get(endorsement.content.index as usize) == Some(&creator)
                {
                    Some(Ok((*endo_id, endorsement.clone())))
                } else {
                    None
                }
            })
            .collect::<Result<Vec<(EndorsementId, Endorsement)>, PoolError>>()?;
        candidates.sort_unstable_by_key(|(_e_id, endo)| endo.content.index);
        candidates.dedup_by_key(|(_e_id, endo)| endo.content.index);
        Ok(candidates)
    }

    /// Incoming endorsements. Returns newly added.
    /// Prunes the pool if there are too many endorsements
    pub fn add_endorsements(
        &mut self,
        endorsements: EndorsementHashMap<Endorsement>,
    ) -> Result<EndorsementHashSet, PoolError> {
        let mut newly_added = EndorsementHashSet::default();
        for (endorsement_id, endorsement) in endorsements.into_iter() {
            massa_trace!("pool add_endorsements endorsement", {
                "endorsement": endorsement
            });

            // Already present
            if self.endorsements.contains_key(&endorsement_id) {
                massa_trace!("pool add_endorsement endorsement already present", {});
                continue;
            }

            // too old
            if endorsement.content.slot.period
                < self.latest_final_periods[endorsement.content.slot.thread as usize]
            {
                massa_trace!("pool add_endorsement endorsement too old", {});
                continue;
            }

            self.endorsements
                .insert(endorsement_id.clone(), endorsement);
            newly_added.insert(endorsement_id);
        }

        // remove excess endorsements
        let removed = self.prune();
        for id in removed.into_iter() {
            newly_added.remove(&id);
        }

        Ok(newly_added)
    }

    pub fn update_current_slot(&mut self, slot: Slot) {
        self.current_slot = Some(slot);
    }

    /// Prune the pool while there are more endorsements than set max
    /// Kept endorsements are the one that are absolutely closer to the current slot
    fn prune(&mut self) -> EndorsementHashSet {
        let mut removed = EndorsementHashSet::default();

        if self.endorsements.len() > self.pool_settings.max_endorsement_count as usize {
            let excess =
                self.endorsements.len() - self.pool_settings.max_endorsement_count as usize;
            let mut candidates: Vec<_> = self.endorsements.clone().into_iter().collect();
            let thread_count = self.latest_final_periods.len() as u8;
            let current_slot_index = self.current_slot.map_or(0u64, |s| {
                s.period
                    .saturating_mul(thread_count as u64)
                    .saturating_add(s.thread as u64)
            });
            candidates.sort_unstable_by_key(|(_, e)| {
                let slot_index = e
                    .content
                    .slot
                    .period
                    .saturating_mul(thread_count as u64)
                    .saturating_add(e.content.slot.thread as u64);
                let abs_diff = if slot_index >= current_slot_index {
                    slot_index - current_slot_index
                } else {
                    current_slot_index - slot_index
                };
                std::cmp::Reverse(abs_diff)
            });
            candidates.truncate(excess);
            for (c_id, _) in candidates.into_iter() {
                self.endorsements.remove(&c_id);
                removed.insert(c_id);
            }
        }
        massa_trace!("pool.endorsement_pool.prune", { "removed": removed });
        removed
    }
}
