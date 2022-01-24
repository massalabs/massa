use std::collections::{btree_map, BTreeMap};

use massa_models::{prehash::Set, Address};
use serde::{Deserialize, Serialize};

use crate::{error::ProofOfStakeError, RollUpdates};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RollCounts(pub BTreeMap<Address, u64>);

impl RollCounts {
    pub fn new() -> Self {
        RollCounts(BTreeMap::new())
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// applies RollUpdates to self with compensations
    pub fn apply_updates(&mut self, updates: &RollUpdates) -> Result<(), ProofOfStakeError> {
        for (addr, update) in updates.0.iter() {
            match self.0.entry(*addr) {
                btree_map::Entry::Occupied(mut occ) => {
                    let cur_val = *occ.get();
                    if update.roll_purchases >= update.roll_sales {
                        *occ.get_mut() = cur_val
                            .checked_add(update.roll_purchases - update.roll_sales)
                            .ok_or_else(|| {
                                ProofOfStakeError::InvalidRollUpdate(
                                    "overflow while incrementing roll count".into(),
                                )
                            })?;
                    } else {
                        *occ.get_mut() = cur_val
                            .checked_sub(update.roll_sales - update.roll_purchases)
                            .ok_or_else(|| {
                                ProofOfStakeError::InvalidRollUpdate(
                                    "underflow while decrementing roll count".into(),
                                )
                            })?;
                    }
                    if *occ.get() == 0 {
                        // remove if 0
                        occ.remove();
                    }
                }
                btree_map::Entry::Vacant(vac) => {
                    if update.roll_purchases >= update.roll_sales {
                        if update.roll_purchases > update.roll_sales {
                            // ignore if 0
                            vac.insert(update.roll_purchases - update.roll_sales);
                        }
                    } else {
                        return Err(ProofOfStakeError::InvalidRollUpdate(
                            "underflow while decrementing roll count".into(),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// get roll counts for a subset of addresses.
    #[must_use]
    pub fn clone_subset(&self, addrs: &Set<Address>) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, *v)))
                .collect(),
        )
    }

    /// merge another roll counts into self, overwriting existing data
    /// addrs that are in not other are removed from self
    pub fn sync_from(&mut self, addrs: &Set<Address>, mut other: RollCounts) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }
}
