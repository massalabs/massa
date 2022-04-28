// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::{
    serialization::{
        u8_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    with_serialization_context,
};
use crate::{constants::SLOT_KEY_SIZE, node_configuration::THREAD_COUNT};
use crate::{error::ModelsError, serialization::DeserializeCompactV2};
use massa_hash::Hash;
use serde::{Deserialize, Serialize};
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

impl PartialOrd for Slot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (self.period, self.thread).partial_cmp(&(other.period, other.thread))
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

    /// returns the minimal slot
    pub const fn min() -> Slot {
        Slot {
            period: 0,
            thread: 0,
        }
    }

    /// returns the maximal slot
    pub const fn max() -> Slot {
        Slot {
            period: u64::MAX,
            thread: u8::MAX,
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

    /// Returns a fixed-size sortable binary key
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::Slot;
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
    /// # use massa_models::Slot;
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
    /// # use massa_models::Slot;
    /// let slot = Slot::new(10,5);
    /// assert_eq!(slot.get_next_slot(5).unwrap(), Slot::new(11, 0))
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
}

use crate::serialization::DeserializeVarIntV2;
use nom::{bytes::complete::take, error::context, sequence::tuple, IResult};

fn take_thread_number(s: &[u8]) -> IResult<&[u8], u8> {
    match take(1usize)(s) {
        Ok((rest, v)) => {
            let thread_number = v[0];
            if thread_number < THREAD_COUNT {
                Ok((rest, thread_number))
            } else {
                Err(nom::Err::Failure(nom::error::Error::new(
                    s,
                    nom::error::ErrorKind::Fail,
                )))
            }
        }
        Err(e) => Err(e),
    }
}

// HERE
impl DeserializeCompactV2 for Slot {
    fn from_bytes_compact_v2<'a>(buffer: &'a [u8]) -> IResult<&[u8], Self> {
        tuple((
            context("period", |buffer: &'a [u8]| {
                u64::from_varint_bytes_v2(buffer, None, None)
            }),
            context("thread", take_thread_number),
        ))(buffer)
        .map(|(rest, (period, thread))| (rest, Slot::new(period, thread)))
    }
}

#[cfg(test)]
mod tests {
    use crate::{serialization::DeserializeCompactV2, slot::Slot, SerializeCompact};

    #[test]
    fn test_new_deserialize() {
        // Test that the serialize and deserialize works and there is no rest
        let slot = Slot::new(3000000, 1);
        let mut serialized = slot.to_bytes_compact().unwrap();
        let deserialized = Slot::from_bytes_compact_v2(&serialized).unwrap();
        assert_eq!(slot, deserialized.1);
        assert!(deserialized.0.is_empty());

        // Test the rest
        serialized.extend(slot.to_bytes_compact().unwrap());
        let deserialized = Slot::from_bytes_compact_v2(&serialized).unwrap();
        assert_eq!(slot, deserialized.1);
        assert!(!deserialized.0.is_empty());
        let rest_deserialized = Slot::from_bytes_compact_v2(&deserialized.0).unwrap();
        assert_eq!(slot, rest_deserialized.1);
        assert!(rest_deserialized.0.is_empty());

        // Test error
        let slot = Slot::new(3000000, 43);
        let serialized = slot.to_bytes_compact().unwrap();
        Slot::from_bytes_compact_v2(&serialized).expect_err("");
    }
}

impl SerializeCompact for Slot {
    /// Returns a compact binary representation of the slot
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::Slot;
    /// # use massa_models::{DeserializeCompact, SerializeCompact};
    /// # massa_models::init_serialization_context(massa_models::SerializationContext::default());
    /// # let context = massa_models::get_serialization_context();
    /// let slot = Slot::new(10,1);
    /// let ser = slot.to_bytes_compact().unwrap();
    /// let (deser, _) = Slot::from_bytes_compact(&ser).unwrap();
    /// assert_eq!(slot, deser);
    /// ```
    ///
    /// Checks performed: none.
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::with_capacity(9);
        res.extend(self.period.to_varint_bytes());
        res.push(self.thread);
        Ok(res)
    }
}

impl DeserializeCompact for Slot {
    /// Deserializes from a compact representation
    ///
    /// ## Example
    /// ```rust
    /// # use massa_models::Slot;
    /// # use massa_models::{DeserializeCompact, SerializeCompact};
    /// # massa_models::init_serialization_context(massa_models::SerializationContext::default());
    /// # let context = massa_models::get_serialization_context();
    /// let slot = Slot::new(10,1);
    /// let ser = slot.to_bytes_compact().unwrap();
    /// let (deser, _) = Slot::from_bytes_compact(&ser).unwrap();
    /// assert_eq!(slot, deser);
    /// ```
    ///
    /// Checks performed:
    /// - Valid period and delta.
    /// - Valid thread.
    /// - Valid thread number.
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let parent_count = with_serialization_context(|context| context.thread_count);
        let mut cursor = 0usize;
        let (period, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        let thread = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        if thread >= parent_count {
            return Err(ModelsError::DeserializeError(
                "invalid thread number".into(),
            ));
        }
        Ok((Slot { period, thread }, cursor))
    }
}
