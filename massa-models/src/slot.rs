// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use massa_hash::Hash;
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::bytes::complete::take;
use nom::error::{context, ContextError, ParseError};
use serde::{Deserialize, Serialize};
use std::ops::{Bound, RangeBounds};
use std::str::FromStr;
use std::{cmp::Ordering, convert::TryInto};

/// a point in time where a block is expected
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// period
    pub period: u64,
    /// thread
    pub thread: u8,
}

/// size of the slot key representation
pub const SLOT_KEY_SIZE: usize = 9;

/// Basic serializer for `Slot`
#[derive(Clone)]
pub struct SlotSerializer {
    u64_serializer: U64VarIntSerializer,
}

impl SlotSerializer {
    /// Creates a `SlotSerializer`
    pub const fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Default for SlotSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Slot> for SlotSerializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::Serializer;
    /// use massa_models::slot::{Slot, SlotSerializer};
    ///
    /// let slot: Slot = Slot::new(1, 3);
    /// let mut serialized = Vec::new();
    /// let serializer = SlotSerializer::new();
    /// serializer.serialize(&slot, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &Slot, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.u64_serializer.serialize(&value.period, buffer)?;
        buffer.push(value.thread);
        Ok(())
    }
}

/// Basic `Slot` Deserializer
#[derive(Clone)]
pub struct SlotDeserializer {
    period_deserializer: U64VarIntDeserializer,
    range_thread: (Bound<u8>, Bound<u8>),
}

impl SlotDeserializer {
    /// Creates a `SlotDeserializer`
    pub const fn new(
        range_period: (Bound<u64>, Bound<u64>),
        range_thread: (Bound<u8>, Bound<u8>),
    ) -> Self {
        Self {
            period_deserializer: U64VarIntDeserializer::new(range_period.0, range_period.1),
            range_thread,
        }
    }
}

impl Deserializer<Slot> for SlotDeserializer {
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::slot::{Slot, SlotSerializer, SlotDeserializer};
    ///
    /// let slot: Slot = Slot::new(1, 3);
    /// let mut serialized = Vec::new();
    /// let serializer = SlotSerializer::new();
    /// let deserializer = SlotDeserializer::new((Included(u64::MIN), Included(u64::MAX)), (Included(u8::MIN), Included(u8::MAX.into())));
    /// serializer.serialize(&slot, &mut serialized).unwrap();
    /// let (rest, slot_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(slot, slot_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> nom::IResult<&'a [u8], Slot, E> {
        context("Failed Slot deserialization", |input: &'a [u8]| {
            let (rest, period) = self.period_deserializer.deserialize(input)?;
            let (rest2, thread_) = take(1usize)(rest)?;
            let thread = thread_[0];
            if !self.range_thread.contains(&thread) {
                return Err(nom::Err::Error(ParseError::from_error_kind(
                    &rest[0..1],
                    nom::error::ErrorKind::Digit,
                )));
            }
            // Safe because we throw just above if there is no character.
            Ok((rest2, Slot { period, thread }))
        })(buffer)
    }
}

impl PartialOrd for Slot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some((self.period, self.thread).cmp(&(other.period, other.thread)))
    }
}

impl Ord for Slot {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.period, self.thread).cmp(&(other.period, other.thread))
    }
}

impl std::fmt::Display for Slot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "(period: {}, thread: {})", self.period, self.thread)?;
        Ok(())
    }
}

impl FromStr for Slot {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let v: Vec<_> = s.split(',').collect();
        if v.len() != 2 {
            Err(ModelsError::DeserializeError(
                "invalid slot format".to_string(),
            ))
        } else {
            Ok(Slot::new(
                v[0].parse::<u64>()
                    .map_err(|_| ModelsError::DeserializeError("invalid period".to_string()))?,
                v[1].parse::<u8>()
                    .map_err(|_| ModelsError::DeserializeError("invalid thread".to_string()))?,
            ))
        }
    }
}

impl Slot {
    /// new slot from period and thread
    pub fn new(period: u64, thread: u8) -> Slot {
        Slot { period, thread }
    }

    /// create the last slot of a given cycle
    pub fn new_last_of_cycle(
        cycle: u64,
        periods_per_cycle: u64,
        thread_count: u8,
    ) -> Result<Slot, ModelsError> {
        let period = cycle
            .checked_mul(periods_per_cycle)
            .ok_or(ModelsError::PeriodOverflowError)?
            .checked_add(periods_per_cycle.saturating_sub(1))
            .ok_or(ModelsError::PeriodOverflowError)?;
        Ok(Slot {
            period,
            thread: thread_count.saturating_sub(1),
        })
    }

    /// create the first slot of a given cycle
    pub fn new_first_of_cycle(cycle: u64, periods_per_cycle: u64) -> Result<Slot, ModelsError> {
        let period = cycle
            .checked_mul(periods_per_cycle)
            .ok_or(ModelsError::PeriodOverflowError)?;
        Ok(Slot { period, thread: 0 })
    }

    /// returns the minimal slot
    pub const fn min() -> Slot {
        Slot {
            period: 0,
            thread: 0,
        }
    }

    /// returns the maximal slot
    pub const fn max(thread_count: u8) -> Slot {
        Slot {
            period: u64::MAX,
            thread: thread_count.saturating_sub(1),
        }
    }

    /// first bit of the slot, for seed purpose
    pub fn get_first_bit(&self) -> bool {
        Hash::compute_from(&self.to_bytes_key()).to_bytes()[0] >> 7 == 1
    }

    /// cycle associated to that slot
    pub fn get_cycle(&self, periods_per_cycle: u64) -> u64 {
        self.period / periods_per_cycle
    }

    /// check if the slot is last in the cycle
    pub fn is_last_of_cycle(&self, periods_per_cycle: u64, thread_count: u8) -> bool {
        self.period % periods_per_cycle == (periods_per_cycle.saturating_sub(1))
            && self.thread == (thread_count.saturating_sub(1))
    }

    /// check if the slot is first in the cycle
    pub fn is_first_of_cycle(&self, periods_per_cycle: u64) -> bool {
        self.period % periods_per_cycle == 0 && self.thread == 0
    }

    /// Returns a fixed-size sortable binary key
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::slot::Slot;
    /// let slot = Slot::new(10,5);
    /// let key = slot.to_bytes_key();
    /// let res = Slot::from_bytes_key(&key);
    /// assert_eq!(slot, res);
    /// ```
    pub fn to_bytes_key(&self) -> [u8; SLOT_KEY_SIZE] {
        let mut res = [0u8; SLOT_KEY_SIZE];
        res[..8].clone_from_slice(&self.period.to_be_bytes());
        res[8] = self.thread;
        res
    }

    /// Deserializes a slot from its fixed-size sortable binary key representation
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::slot::Slot;
    /// let slot = Slot::new(10,5);
    /// let key = slot.to_bytes_key();
    /// let res = Slot::from_bytes_key(&key);
    /// assert_eq!(slot, res);
    /// ```
    pub fn from_bytes_key(buffer: &[u8; SLOT_KEY_SIZE]) -> Self {
        Slot {
            period: u64::from_be_bytes(buffer[..8].try_into().unwrap()), // cannot fail
            thread: buffer[8],
        }
    }

    /// Returns the next Slot
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::slot::Slot;
    /// let slot = Slot::new(10,3);
    /// assert_eq!(slot.get_next_slot(5).unwrap(), Slot::new(10, 4));
    /// let slot = Slot::new(10,4);
    /// assert_eq!(slot.get_next_slot(5).unwrap(), Slot::new(11, 0));
    /// ```
    pub fn get_next_slot(&self, thread_count: u8) -> Result<Slot, ModelsError> {
        if self.thread.saturating_add(1u8) >= thread_count {
            Ok(Slot::new(
                self.period
                    .checked_add(1u64)
                    .ok_or(ModelsError::PeriodOverflowError)?,
                0u8,
            ))
        } else {
            Ok(Slot::new(
                self.period,
                self.thread
                    .checked_add(1u8)
                    .ok_or(ModelsError::ThreadOverflowError)?,
            ))
        }
    }

    /// Returns the previous Slot
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::slot::Slot;
    /// let slot = Slot::new(10,1);
    /// assert_eq!(slot.get_prev_slot(5).unwrap(), Slot::new(10, 0));
    /// let slot = Slot::new(10,0);
    /// assert_eq!(slot.get_prev_slot(5).unwrap(), Slot::new(9, 4));
    /// ```
    pub fn get_prev_slot(&self, thread_count: u8) -> Result<Slot, ModelsError> {
        match self.thread.checked_sub(1u8) {
            Some(t) => Ok(Slot::new(self.period, t)),
            None => Ok(Slot::new(
                self.period
                    .checked_sub(1)
                    .ok_or(ModelsError::PeriodOverflowError)?,
                thread_count.saturating_sub(1),
            )),
        }
    }

    /// Counts the number of slots since the one passed in parameter and until self
    /// If the two slots are equal, the returned value is `0`.
    /// If the passed slot is strictly higher than self, an error is returned
    pub fn slots_since(&self, s: &Slot, thread_count: u8) -> Result<u64, ModelsError> {
        // if s > self, return an error
        if s > self {
            return Err(ModelsError::PeriodOverflowError);
        }

        // compute the number of slots from s to self
        Ok((self.period - s.period)
            .checked_mul(thread_count as u64)
            .ok_or(ModelsError::PeriodOverflowError)?
            .checked_add(self.thread as u64)
            .ok_or(ModelsError::PeriodOverflowError)?
            .saturating_sub(s.thread as u64))
    }

    /// Returns the n-th slot after the current one
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::slot::Slot;
    /// let slot = Slot::new(10,3);
    /// assert_eq!(slot.skip(62, 32).unwrap(), Slot::new(12, 1));
    /// ```
    pub fn skip(&self, n: u64, thread_count: u8) -> Result<Slot, ModelsError> {
        let mut res_period = self
            .period
            .checked_add(n / (thread_count as u64))
            .ok_or(ModelsError::PeriodOverflowError)?;
        let mut res_thread = (self.thread as u64)
            .checked_add(n % (thread_count as u64))
            .ok_or(ModelsError::PeriodOverflowError)?;

        if res_thread >= thread_count as u64 {
            res_period = res_period
                .checked_add(1)
                .ok_or(ModelsError::PeriodOverflowError)?;
            res_thread -= thread_count as u64;
        }

        Ok(Slot::new(res_period, res_thread as u8))
    }
}

/// When an address is drawn to create an endorsement it is selected for a specific index
#[derive(Debug, Clone, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub struct IndexedSlot {
    /// slot
    pub slot: Slot,
    /// endorsement index in the slot
    pub index: usize,
}

impl std::fmt::Display for IndexedSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Slot: {}, Index: {}", self.slot, self.index)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_new_last_of_cycle() {
        // Test case 1: Valid inputs
        let expected_slot = Slot {
            period: 767,
            thread: 31,
        };
        let actual_slot = Slot::new_last_of_cycle(5, 128, 32).unwrap();
        assert_eq!(actual_slot, expected_slot);

        // Test case 2: Overflow scenario for period multiplication
        let expected_error_mul = "period overflow error".to_string();
        let actual_error_overflow_mul = Slot::new_last_of_cycle(u64::MAX, 128, 32)
            .unwrap_err()
            .to_string();

        assert_eq!(actual_error_overflow_mul, expected_error_mul);

        // Test case 3: Overflow scenario for period addition
        let actual_error_overflow_add = Slot::new_last_of_cycle(u64::MAX - 1, u64::MAX, 32)
            .unwrap_err()
            .to_string();

        assert_eq!(actual_error_overflow_add, expected_error_mul);
    }

    #[test]
    fn test_new_first_of_cycle() {
        // Test case 1: Valid inputs for new_first_of_cycle
        let expected_slot = Slot {
            period: 640,
            thread: 0,
        };
        let actual_slot = Slot::new_first_of_cycle(5, 128).unwrap();
        assert_eq!(actual_slot, expected_slot);

        // Test case 2: Overflow scenario for new_first_of_cycle
        let expected_error = "period overflow error".to_string();
        let actual_error = Slot::new_first_of_cycle(u64::MAX, 128)
            .unwrap_err()
            .to_string();

        assert_eq!(actual_error, expected_error);
    }

    #[test]
    fn test_slot_serde() {
        let expected_slot = Slot::new(12, 32);

        let serialized = serde_json::to_string(&expected_slot).unwrap();
        let actual_slot: Slot = serde_json::from_str(&serialized).unwrap();

        assert_eq!(actual_slot, expected_slot);
    }
}
