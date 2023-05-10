use crate::{
    address::Address,
    error::ModelsError,
    error::ModelsResult as Result,
    prehash::{PreHashMap, PreHashSet},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map;
use std::ops::Bound::Included;

use std::collections::{btree_map, BTreeMap};

/// just a `u64` to keep track of the roll sells and buys during a cycle
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RollCompensation(pub u64);

/// roll sales and purchases
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RollUpdate {
    /// roll purchases
    pub(crate) roll_purchases: u64,
    /// roll sales
    pub(crate) roll_sales: u64,
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
    pub(crate) fn compensate(&mut self) -> RollCompensation {
        let compensation = std::cmp::min(self.roll_purchases, self.roll_sales);
        self.roll_purchases -= compensation;
        self.roll_sales -= compensation;
        RollCompensation(compensation)
    }

    /// true if the update has no effect
    pub(crate) fn is_nil(&self) -> bool {
        self.roll_purchases == 0 && self.roll_sales == 0
    }
}

/// Serializer for `RollUpdate`
pub(crate) struct RollUpdateSerializer {
    u64_serializer: U64VarIntSerializer,
}

impl RollUpdateSerializer {
    /// Creates a new `RollUpdateSerializer`
    pub(crate) fn new() -> Self {
        RollUpdateSerializer {
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Default for RollUpdateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<RollUpdate> for RollUpdateSerializer {
    /// ## Example:
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_models::rolls::{RollUpdate, RollUpdateSerializer};
    /// use massa_serialization::Serializer;
    ///
    /// let roll_update = RollUpdate {
    ///   roll_purchases: 1,
    ///   roll_sales: 2,
    /// };
    /// let mut buffer = vec![];
    /// RollUpdateSerializer::new().serialize(&roll_update, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &RollUpdate, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.u64_serializer
            .serialize(&value.roll_purchases, buffer)?;
        self.u64_serializer.serialize(&value.roll_sales, buffer)?;
        Ok(())
    }
}

/// Deserializer for `RollUpdate`
pub(crate) struct RollUpdateDeserializer {
    u64_deserializer: U64VarIntDeserializer,
}

impl RollUpdateDeserializer {
    /// Creates a new `RollUpdateDeserializer`
    pub(crate) fn new() -> Self {
        RollUpdateDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        }
    }
}

impl Default for RollUpdateDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<RollUpdate> for RollUpdateDeserializer {
    /// ## Example:
    /// ```rust,ignore
    /// // TODO: reinstate this doc-test. was ignored when these were made private
    /// use massa_models::rolls::{RollUpdate, RollUpdateDeserializer, RollUpdateSerializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let roll_update = RollUpdate {
    ///   roll_purchases: 1,
    ///   roll_sales: 2,
    /// };
    /// let mut buffer = vec![];
    /// RollUpdateSerializer::new().serialize(&roll_update, &mut buffer).unwrap();
    /// let (rest, roll_update_deserialized) = RollUpdateDeserializer::new().deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(roll_update.roll_purchases, roll_update_deserialized.roll_purchases);
    /// assert_eq!(roll_update.roll_sales, roll_update_deserialized.roll_sales);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], RollUpdate, E> {
        context(
            "Failed RollUpdate deserialization",
            tuple((
                context("Failed roll_purchases deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed roll_sales deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(roll_purchases, roll_sales)| RollUpdate {
            roll_purchases,
            roll_sales,
        })
        .parse(buffer)
    }
}

/// maps addresses to roll updates
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct RollUpdates(pub PreHashMap<Address, RollUpdate>);

impl RollUpdates {
    /// the addresses impacted by the updates
    pub(crate) fn get_involved_addresses(&self) -> PreHashSet<Address> {
        self.0.keys().copied().collect()
    }

    /// chains with another `RollUpdates`, compensates and returns compensations
    pub(crate) fn chain(
        &mut self,
        updates: &RollUpdates,
    ) -> Result<PreHashMap<Address, RollCompensation>> {
        let mut res = PreHashMap::default();
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

    /// applies a `RollUpdate`, compensates and returns compensation
    pub(crate) fn apply(
        &mut self,
        addr: &Address,
        update: &RollUpdate,
    ) -> Result<RollCompensation> {
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
    pub(crate) fn clone_subset(&self, addrs: &PreHashSet<Address>) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, v.clone())))
                .collect(),
        )
    }

    /// merge another roll updates into self, overwriting existing data
    /// addresses that are in not other are removed from self
    pub(crate) fn sync_from(&mut self, addrs: &PreHashSet<Address>, mut other: RollUpdates) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }
}

/// counts the roll for each address
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub(crate) struct RollCounts(pub BTreeMap<Address, u64>);

impl RollCounts {
    /// Makes a new, empty `RollCounts`.
    pub(crate) fn new() -> Self {
        RollCounts(BTreeMap::new())
    }

    /// Returns the number of elements in the `RollCounts`.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the `RollCounts` contains no elements.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// applies `RollUpdates` to self with compensations
    pub(crate) fn apply_updates(&mut self, updates: &RollUpdates) -> Result<()> {
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
    pub(crate) fn clone_subset(&self, addrs: &PreHashSet<Address>) -> Self {
        Self(
            addrs
                .iter()
                .filter_map(|addr| self.0.get(addr).map(|v| (*addr, *v)))
                .collect(),
        )
    }

    /// merge another roll counts into self, overwriting existing data
    /// addresses that are in not other are removed from self
    pub(crate) fn sync_from(&mut self, addrs: &PreHashSet<Address>, mut other: RollCounts) {
        for addr in addrs.iter() {
            if let Some(new_val) = other.0.remove(addr) {
                self.0.insert(*addr, new_val);
            } else {
                self.0.remove(addr);
            }
        }
    }
}
