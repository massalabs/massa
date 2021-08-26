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

        // prune excess endorsements
        let removed = self.prune();
        for id in removed {
            newly_added.remove(&id);
        }

        Ok(newly_added)
    }

    pub fn update_current_slot(&mut self, slot: Slot) {
        self.current_slot = Some(slot);
        self.prune();
    }

    fn prune(&mut self) -> Vec<EndorsementId> {
        let mut removed = Vec::new();
        if self.endorsements.len() > self.cfg.max_endorsement_count as usize {
            let excess = self.endorsements.len() - self.cfg.max_endorsement_count as usize;
            let candidates = self.endorsements.clone();
            let mut candidates: Vec<_> = candidates.into_iter().collect();
            candidates.sort_unstable_by_key(|(_, e)| {
                let slot = e.content.slot;
                let thread_count = self.latest_final_periods.len();
                let current_slot = self.current_slot.unwrap_or_else(|| Slot::new(0, 0));
                num::abs_sub(
                    slot.thread as i64 / thread_count as i64 + slot.period as i64,
                    current_slot.thread as i64 / thread_count as i64 + current_slot.period as i64,
                )
            });
            for (id, _) in candidates[0..excess].into_iter() {
                self.endorsements.remove(id);
                removed.push(*id);
            }
        }
        removed
    }
}
