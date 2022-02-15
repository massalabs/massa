use serde::{Deserialize, Serialize};
use std::collections::hash_map;

use crate::{
    error::ModelsResult as Result,
    prehash::{Map, Set},
    Address, DeserializeCompact, DeserializeVarInt, ModelsError, SerializeCompact, SerializeVarInt,
};

use std::collections::{btree_map, BTreeMap};

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct RollCompensation(pub u64);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RollUpdate {
    pub roll_purchases: u64,
    pub roll_sales: u64,
    // Here is space for registering any denunciations/resets
}

impl RollUpdate {
    /// chain two roll updates, compensate and return compensation count
    fn chain(&mut self, change: &Self) -> Result<RollCompensation> {
        let compensation_other = std::cmp::min(change.roll_purchases, change.roll_sales);
        self.roll_purchases = self
            .roll_purchases
            .checked_add(change.roll_purchases - compensation_other)
            .ok_or_else(|| {
                ModelsError::InvalidRollUpdate(
                    "roll_purchases overflow in RollUpdate::chain".into(),
                )
            })?;
        self.roll_sales = self
            .roll_sales
            .checked_add(change.roll_sales - compensation_other)
            .ok_or_else(|| {
                ModelsError::InvalidRollUpdate("roll_sales overflow in RollUpdate::chain".into())
            })?;

        let compensation_self = self.compensate().0;

        let compensation_total = compensation_other
            .checked_add(compensation_self)
            .ok_or_else(|| {
                ModelsError::InvalidRollUpdate("compensation overflow in RollUpdate::chain".into())
            })?;
        Ok(RollCompensation(compensation_total))
    }

    /// compensate a roll update, return compensation count
    pub fn compensate(&mut self) -> RollCompensation {
        let compensation = std::cmp::min(self.roll_purchases, self.roll_sales);
        self.roll_purchases -= compensation;
        self.roll_sales -= compensation;
        RollCompensation(compensation)
    }

    pub fn is_nil(&self) -> bool {
        self.roll_purchases == 0 && self.roll_sales == 0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct RollUpdates(pub Map<Address, RollUpdate>);

impl RollUpdates {
    pub fn get_involved_addresses(&self) -> Set<Address> {
        self.0.keys().copied().collect()
    }

    /// chains with another RollUpdates, compensates and returns compensations
    pub fn chain(&mut self, updates: &RollUpdates) -> Result<Map<Address, RollCompensation>> {
        let mut res = Map::default();
        for (addr, update) in updates.0.iter() {
            res.insert(*addr, self.apply(addr, update)?);
            // remove if nil
            if let hash_map::Entry::Occupied(occ) = self.0.entry(*addr) {
                if occ.get().is_nil() {
                    occ.remove();
                }
            }
        }
        Ok(res)
    }

    /// applies a RollUpdate, compensates and returns compensation
    pub fn apply(&mut self, addr: &Address, update: &RollUpdate) -> Result<RollCompensation> {
        if update.is_nil() {
            return Ok(RollCompensation(0));
        }
        match self.0.entry(*addr) {
            hash_map::Entry::Occupied(mut occ) => occ.get_mut().chain(update),
            hash_map::Entry::Vacant(vac) => {
                let mut compensated_update = update.clone();
                let compensation = compensated_update.compensate();
                vac.insert(compensated_update);
                Ok(compensation)
            }
        }
    }

    /// get the roll update for a subset of addresses
    #[must_use]
    pub fn clone_subset(&self, addrs: &Set<Address>) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, v.clone())))
                .collect(),
        )
    }

    /// merge another roll updates into self, overwriting existing data
    /// addrs that are in not other are removed from self
    pub fn sync_from(&mut self, addrs: &Set<Address>, mut other: RollUpdates) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }
}

impl SerializeCompact for RollUpdate {
    fn to_bytes_compact(&self) -> Result<Vec<u8>> {
        let mut res: Vec<u8> = Vec::new();

        // roll purchases
        res.extend(self.roll_purchases.to_varint_bytes());

        // roll sales
        res.extend(self.roll_sales.to_varint_bytes());

        Ok(res)
    }
}

impl DeserializeCompact for RollUpdate {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize)> {
        let mut cursor = 0usize;

        // roll purchases
        let (roll_purchases, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // roll sales
        let (roll_sales, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        Ok((
            RollUpdate {
                roll_purchases,
                roll_sales,
            },
            cursor,
        ))
    }
}

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
    pub fn apply_updates(&mut self, updates: &RollUpdates) -> Result<()> {
        for (addr, update) in updates.0.iter() {
            match self.0.entry(*addr) {
                btree_map::Entry::Occupied(mut occ) => {
                    let cur_val = *occ.get();
                    if update.roll_purchases >= update.roll_sales {
                        *occ.get_mut() = cur_val
                            .checked_add(update.roll_purchases - update.roll_sales)
                            .ok_or_else(|| {
                                ModelsError::InvalidRollUpdate(
                                    "overflow while incrementing roll count".into(),
                                )
                            })?;
                    } else {
                        *occ.get_mut() = cur_val
                            .checked_sub(update.roll_sales - update.roll_purchases)
                            .ok_or_else(|| {
                                ModelsError::InvalidRollUpdate(
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
                        return Err(ModelsError::InvalidRollUpdate(
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
