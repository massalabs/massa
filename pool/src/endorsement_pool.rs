// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{PoolConfig, PoolError};
use models::{Address, BlockId, Endorsement, EndorsementContent, EndorsementId, Slot};
use std::collections::{HashMap, HashSet};

pub struct EndorsementPool {
    endorsements: HashMap<EndorsementId, Endorsement>,
    latest_final_periods: Vec<u64>,
    current_slot: Option<Slot>,
    cfg: PoolConfig,
}

impl EndorsementPool {
    pub fn new(cfg: PoolConfig, thread_count: u8) -> EndorsementPool {
        EndorsementPool {
            endorsements: Default::default(),
            cfg,
            current_slot: None,
            latest_final_periods: vec![0; thread_count as usize],
        }
    }

    /// Removes endorsements that are too old
    pub fn update_latest_final_periods(&mut self, periods: Vec<u64>) {
        self.latest_final_periods = periods.clone();
        self.endorsements.retain(
            |_,
             Endorsement {
                 content: EndorsementContent { slot, .. },
                 ..
             }| periods[slot.thread as usize] >= slot.period,
        );
        self.latest_final_periods = periods;
    }

    pub fn get_endorsements(
        &self,
        target_slot: Slot,
        parent: BlockId,
        creators: Vec<Address>,
    ) -> Result<Vec<Endorsement>, PoolError> {
        let mut candidates = self
            .endorsements
            .iter()
            .filter_map(|(_, endorsement)| {
                let creator = match Address::from_public_key(&endorsement.content.sender_public_key)
                {
                    Ok(addr) => addr,
                    Err(e) => return Some(Err(e.into())),
                };
                if endorsement.content.endorsed_block == parent
                    && endorsement.content.slot == target_slot
                    && creators.get(endorsement.content.index as usize) == Some(&creator)
                {
                    Some(Ok(endorsement.clone()))
                } else {
                    None
                }
            })
            .collect::<Result<Vec<Endorsement>, PoolError>>()?;
        candidates.sort_unstable_by_key(|endo| endo.content.index);
        candidates.dedup_by_key(|e| e.content.index);
        Ok(candidates)
    }

    /// Incoming endorsements. Returns newly added
    pub fn add_endorsements(
        &mut self,
        endorsements: HashMap<EndorsementId, Endorsement>,
    ) -> Result<HashSet<EndorsementId>, PoolError> {
        let mut newly_added = HashSet::new();
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

    fn prune(&mut self) -> HashSet<EndorsementId> {
        let mut removed = HashSet::new();

        if self.endorsements.len() > self.cfg.max_endorsement_count as usize {
            let excess = self.endorsements.len() - self.cfg.max_endorsement_count as usize;
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
