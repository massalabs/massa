// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::{PoolConfig, PoolError};
use models::{BlockId, Endorsement, EndorsementContent, EndorsementId, Slot};
use std::collections::{HashMap, HashSet};

pub struct EndorsementPool {
    endorsements: HashMap<EndorsementId, Endorsement>,
    cfg: PoolConfig,
}

impl EndorsementPool {
    pub fn new(cfg: PoolConfig) -> EndorsementPool {
        EndorsementPool {
            endorsements: Default::default(),
            cfg,
        }
    }

    /// Removes endorsements that are too old
    pub fn update_latest_final_periods(&mut self, periods: Vec<u64>) {
        self.endorsements.drain_filter(
            |_,
             Endorsement {
                 content: EndorsementContent { slot, .. },
                 ..
             }| periods[slot.thread as usize] > slot.period,
        );
    }

    pub fn get_endorsement(&self, target_slot: Slot, parent: BlockId) -> Vec<Endorsement> {
        self.endorsements
            .iter()
            .filter_map(|(_, endorsement)| {
                if endorsement.content.endorsed_block == parent.0
                    && endorsement.content.slot == target_slot
                {
                    Some(endorsement.clone())
                } else {
                    None
                }
            })
            .collect()
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

            self.endorsements
                .insert(endorsement_id.clone(), endorsement);
            newly_added.insert(endorsement_id);
        }
        Ok(newly_added)
    }
}
