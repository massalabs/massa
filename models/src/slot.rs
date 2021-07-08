use super::{
    context::SerializationContext,
    serialization::{
        u8_from_slice, DeserializeCompact, DeserializeVarInt, SerializeCompact, SerializeVarInt,
    },
};
use crate::error::ModelsError;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, convert::TryInto, fmt};

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

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "(period: {}, thread: {})", self.period, self.thread)
    }
}

impl Slot {
    pub fn new(period: u64, thread: u8) -> Slot {
        Slot { period, thread }
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
    /// # use models::SerializationContext;
    /// # use models::{DeserializeCompact, SerializeCompact};
    /// #  let context = SerializationContext {
    /// #      max_block_size: 100000,
    /// #      max_block_operations: 1000000,
    /// #      parent_count: 4,
    /// #      max_peer_list_length: 128,
    /// #      max_message_size: 3 * 1024 * 1024,
    /// #      max_bootstrap_blocks: 100,
    /// #      max_bootstrap_cliques: 100,
    /// #      max_bootstrap_deps: 100,
    /// #      max_bootstrap_children: 100,
    /// #      max_ask_blocks_per_message: 10,
    /// #      max_bootstrap_message_size: 100000000,
    /// #  };
    /// let slot = Slot::new(10,3);
    /// let ser = slot.to_bytes_compact(&context).unwrap();
    /// let (deser, _) = Slot::from_bytes_compact(&ser, &context).unwrap();
    /// assert_eq!(slot, deser);
    /// ```
    fn to_bytes_compact(&self, _context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::with_capacity(9);
        res.extend(self.period.to_varint_bytes());
        res.push(self.thread);
        Ok(res)
    }
}

impl DeserializeCompact for Slot {
    /// deserializes from a compact representation
    ///
    /// ## Example
    /// ```rust
    /// # use models::Slot;
    /// # use models::SerializationContext;
    /// # use models::{DeserializeCompact, SerializeCompact};
    /// #  let context = SerializationContext {
    /// #      max_block_size: 100000,
    /// #      max_block_operations: 1000000,
    /// #      parent_count: 4,
    /// #      max_peer_list_length: 128,
    /// #      max_message_size: 3 * 1024 * 1024,
    /// #      max_bootstrap_blocks: 100,
    /// #      max_bootstrap_cliques: 100,
    /// #      max_bootstrap_deps: 100,
    /// #      max_bootstrap_children: 100,
    /// #      max_ask_blocks_per_message: 10,
    /// #      max_bootstrap_message_size: 100000000,
    /// #  };
    /// let slot = Slot::new(10,3);
    /// let ser = slot.to_bytes_compact(&context).unwrap();
    /// let (deser, _) = Slot::from_bytes_compact(&ser, &context).unwrap();
    /// assert_eq!(slot, deser);
    /// ```
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;
        let (period, delta) = u64::from_varint_bytes(buffer)?;
        cursor += delta;
        let thread = u8_from_slice(&buffer[cursor..])?;
        cursor += 1;
        if thread >= context.parent_count {
            return Err(ModelsError::DeserializeError(
                "invalid thread number".into(),
            ));
        }
        Ok((Slot { period, thread }, cursor))
    }
}
