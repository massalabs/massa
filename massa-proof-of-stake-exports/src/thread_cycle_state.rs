use bitvec::{order::Lsb0, prelude::BitVec};
use massa_models::{
    array_from_slice,
    constants::ADDRESS_SIZE_BYTES,
    prehash::{BuildMap, Map},
    rolls::{RollCounts, RollUpdate, RollUpdates},
    with_serialization_context, Address, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializeCompact, SerializeVarInt, Slot,
};
use serde::{Deserialize, Serialize};

/// Rolls state for a cycle in a thread
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThreadCycleState {
    /// Cycle number
    pub cycle: u64,
    /// Last final slot (can be a miss)
    pub last_final_slot: Slot,
    /// Number of rolls an address has
    pub roll_count: RollCounts,
    /// Cycle roll updates
    pub cycle_updates: RollUpdates,
    /// Used to seed the random selector at each cycle
    pub rng_seed: BitVec<Lsb0, u8>,
    /// Per-address production statistics `(ok_count, nok_count)`
    pub production_stats: Map<Address, (u64, u64)>,
}

impl ThreadCycleState {
    /// returns true if all slots of this cycle for this thread are final
    pub fn is_complete(&self, periods_per_cycle: u64) -> bool {
        self.last_final_slot.period == (self.cycle + 1) * periods_per_cycle - 1
    }
}

impl SerializeCompact for ThreadCycleState {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // cycle
        res.extend(self.cycle.to_varint_bytes());

        // last final slot
        res.extend(self.last_final_slot.to_bytes_compact()?);

        // roll count
        let n_entries: u32 = self.roll_count.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState roll_count: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        for (addr, n_rolls) in self.roll_count.0.iter() {
            res.extend(addr.to_bytes());
            res.extend(n_rolls.to_varint_bytes());
        }

        // cycle updates
        let n_entries: u32 = self.cycle_updates.0.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState cycle_updates: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        for (addr, updates) in self.cycle_updates.0.iter() {
            res.extend(addr.to_bytes());
            res.extend(updates.to_bytes_compact()?);
        }

        // rng seed
        let n_entries: u32 = self.rng_seed.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState rng_seed: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        res.extend(self.rng_seed.clone().into_vec());

        // production stats
        let n_entries: u32 = self.production_stats.len().try_into().map_err(|err| {
            ModelsError::SerializeError(format!(
                "too many entries when serializing ExportThreadCycleState production_stats: {}",
                err
            ))
        })?;
        res.extend(n_entries.to_varint_bytes());
        for (addr, (ok_count, nok_count)) in self.production_stats.iter() {
            res.extend(addr.to_bytes());
            res.extend(ok_count.to_varint_bytes());
            res.extend(nok_count.to_varint_bytes());
        }

        Ok(res)
    }
}

impl DeserializeCompact for ThreadCycleState {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), ModelsError> {
        let max_entries = with_serialization_context(|context| (context.max_bootstrap_pos_entries));
        let mut cursor = 0usize;

        // cycle
        let (cycle, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // last final slot
        let (last_final_slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // roll count
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat roll_count".into(),
            ));
        }
        let mut roll_count = RollCounts::default();
        for _ in 0..n_entries {
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (rolls, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            roll_count.0.insert(addr, rolls);
        }

        // cycle updates
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat cycle_updates"
                    .into(),
            ));
        }
        let mut cycle_updates = RollUpdates(Map::with_capacity_and_hasher(
            n_entries as usize,
            BuildMap::default(),
        ));
        for _ in 0..n_entries {
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (update, delta) = RollUpdate::from_bytes_compact(&buffer[cursor..])?;
            cursor += delta;
            cycle_updates.0.insert(addr, update);
        }

        // rng seed
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat rng_seed".into(),
            ));
        }
        let bits_u8_len = n_entries.div_ceil(u8::BITS) as usize;
        if buffer[cursor..].len() < bits_u8_len {
            return Err(ModelsError::SerializeError(
                "too few remaining bytes when deserializing ExportThreadCycleStat rng_seed".into(),
            ));
        }
        let mut rng_seed: BitVec<Lsb0, u8> = BitVec::try_from_vec(buffer[cursor..(cursor+bits_u8_len)].to_vec())
            .map_err(|_| ModelsError::SerializeError("error in bitvec conversion during deserialization of ExportThreadCycleStat rng_seed".into()))?;
        rng_seed.truncate(n_entries as usize);
        if rng_seed.len() != n_entries as usize {
            return Err(ModelsError::SerializeError(
                "incorrect resulting size when deserializing ExportThreadCycleStat rng_seed".into(),
            ));
        }
        cursor += rng_seed.elements();

        // production stats
        let (n_entries, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;
        if n_entries > max_entries {
            return Err(ModelsError::SerializeError(
                "invalid number entries when deserializing ExportThreadCycleStat production_stats"
                    .into(),
            ));
        }
        let mut production_stats =
            Map::with_capacity_and_hasher(n_entries as usize, BuildMap::default());
        for _ in 0..n_entries {
            let addr = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
            cursor += ADDRESS_SIZE_BYTES;
            let (ok_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            let (nok_count, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
            cursor += delta;
            production_stats.insert(addr, (ok_count, nok_count));
        }

        // return struct
        Ok((
            ThreadCycleState {
                cycle,
                last_final_slot,
                roll_count,
                cycle_updates,
                rng_seed,
                production_stats,
            },
            cursor,
        ))
    }
}
