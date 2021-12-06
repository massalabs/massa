// Copyright (c) 2021 MASSA LABS <info@massa.net>

use super::{
    serialization::{
        u8_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
    with_serialization_context,
};
use crate::error::ModelsError;
use massa_hash::hash::Hash;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, convert::TryInto};

pub const SLOT_KEY_SIZE: usize = 9;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub period: u64,
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

impl Slot {
    pub fn new(period: u64, thread: u8) -> Slot {
        Slot { period, thread }
    }

    pub fn get_first_bit(&self) -> bool {
        Hash::from(&self.to_bytes_key()).to_bytes()[0] >> 7 == 1
    }

    pub fn get_cycle(&self, periods_per_cycle: u64) -> u64 {
        self.period / periods_per_cycle
    }

    /// Returns a fixed-size sortable binary key
    ///
    /// ## Example
    /// ```rust
    /// # use models::Slot;
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
    /// # use models::Slot;
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
    /// # use models::Slot;
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

impl SerializeCompact for Slot {
    /// Returns a compact binary representation of the slot
    ///
    /// ## Example
    /// ```rust
    /// # use models::Slot;
    /// # use models::{DeserializeCompact, SerializeCompact};
    /// # models::init_serialization_context(models::SerializationContext {
    /// #    max_block_operations: 1024,
    /// #    parent_count: 2,
    /// #    max_peer_list_length: 128,
    /// #    max_message_size: 3 * 1024 * 1024,
    /// #    max_block_size: 3 * 1024 * 1024,
    /// #    max_bootstrap_blocks: 100,
    /// #    max_bootstrap_cliques: 100,
    /// #    max_bootstrap_deps: 100,
    /// #    max_bootstrap_children: 100,
    /// #    max_ask_blocks_per_message: 10,
    /// #    max_operations_per_message: 1024,
    /// #    max_endorsements_per_message: 1024,
    /// #    max_bootstrap_message_size: 100000000,
    /// #     max_bootstrap_pos_cycles: 10000,
    /// #     max_bootstrap_pos_entries: 10000,
    /// #     max_block_endorsements: 8,
    /// # });
    /// # let context = models::get_serialization_context();
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
    /// # use models::Slot;
    /// # use models::{DeserializeCompact, SerializeCompact};
    /// # models::init_serialization_context(models::SerializationContext {
    /// #     max_block_operations: 1024,
    /// #     parent_count: 2,
    /// #     max_peer_list_length: 128,
    /// #     max_message_size: 3 * 1024 * 1024,
    /// #     max_block_size: 3 * 1024 * 1024,
    /// #     max_bootstrap_blocks: 100,
    /// #     max_bootstrap_cliques: 100,
    /// #     max_bootstrap_deps: 100,
    /// #     max_bootstrap_children: 100,
    /// #     max_ask_blocks_per_message: 10,
    /// #     max_endorsements_per_message: 1024,
    /// #     max_operations_per_message: 1024,
    /// #     max_bootstrap_message_size: 100000000,
    /// #     max_bootstrap_pos_cycles: 10000,
    /// #     max_bootstrap_pos_entries: 10000,
    /// #     max_block_endorsements: 8,
    /// # });
    /// # let context = models::get_serialization_context();
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
        let parent_count = with_serialization_context(|context| context.parent_count);
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
