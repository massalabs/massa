use bitvec::prelude::BitVec;
use massa_models::{
    constants::{MAX_BOOTSTRAP_POS_ENTRIES, THREAD_COUNT},
    prehash::Map,
    rolls::{RollCounts, RollUpdateDeserializer, RollUpdateSerializer, RollUpdates},
    Address, AddressDeserializer, Slot, SlotDeserializer, SlotSerializer,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ErrorKind, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;

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
    pub rng_seed: BitVec<u8>,
    /// Per-address production statistics `(ok_count, nok_count)`
    pub production_stats: Map<Address, (u64, u64)>,
}

impl ThreadCycleState {
    /// returns true if all slots of this cycle for this thread are final
    pub fn is_complete(&self, periods_per_cycle: u64) -> bool {
        self.last_final_slot.period == (self.cycle + 1) * periods_per_cycle - 1
    }
}

/// Serializer for `ThreadCycleState`
pub struct ThreadCycleStateSerializer {
    u32_serializer: U32VarIntSerializer,
    u64_serializer: U64VarIntSerializer,
    slot_serializer: SlotSerializer,
    roll_update_serializer: RollUpdateSerializer,
}

impl ThreadCycleStateSerializer {
    /// Creates a new `ThreadCycleStateSerializer`
    pub fn new() -> Self {
        ThreadCycleStateSerializer {
            u32_serializer: U32VarIntSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            slot_serializer: SlotSerializer::new(),
            roll_update_serializer: RollUpdateSerializer::new(),
        }
    }
}

impl Default for ThreadCycleStateSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<ThreadCycleState> for ThreadCycleStateSerializer {
    fn serialize(
        &self,
        value: &ThreadCycleState,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        // cycle
        self.u64_serializer.serialize(&value.cycle, buffer)?;
        // last final slot
        self.slot_serializer
            .serialize(&value.last_final_slot, buffer)?;
        // roll count
        let n_entries: u32 = value.roll_count.0.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many entries when serializing ExportThreadCycleState roll_count: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&n_entries, buffer)?;
        for (address, count) in value.roll_count.0.iter() {
            buffer.extend(address.to_bytes());
            self.u64_serializer.serialize(count, buffer)?;
        }

        // cycle updates
        let n_entries: u32 = value.cycle_updates.0.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many entries when serializing ExportThreadCycleState cycle_updates: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&n_entries, buffer)?;
        for (address, update) in value.cycle_updates.0.iter() {
            buffer.extend(address.to_bytes());
            self.roll_update_serializer.serialize(update, buffer)?;
        }

        // rng seed
        let n_entries: u32 = value.rng_seed.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many entries when serializing ExportThreadCycleState rng_seed: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&n_entries, buffer)?;
        buffer.extend(value.rng_seed.clone().into_vec());

        // production stats
        let n_entries: u32 = value.production_stats.len().try_into().map_err(|err| {
            SerializeError::NumberTooBig(format!(
                "too many entries when serializing ExportThreadCycleState production_stats: {}",
                err
            ))
        })?;
        self.u32_serializer.serialize(&n_entries, buffer)?;
        for (address, (ok_count, nok_count)) in value.production_stats.iter() {
            buffer.extend(address.to_bytes());
            self.u64_serializer.serialize(ok_count, buffer)?;
            self.u64_serializer.serialize(nok_count, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `ThreadCycleState`
pub struct ThreadCycleStateDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    u64_deserializer: U64VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    roll_update_deserializer: RollUpdateDeserializer,
    address_deserializer: AddressDeserializer,
}

impl ThreadCycleStateDeserializer {
    /// Creates a new `ThreadCycleStateDeserializer`
    pub fn new() -> Self {
        ThreadCycleStateDeserializer {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_POS_ENTRIES),
            ),
            u64_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
            roll_update_deserializer: RollUpdateDeserializer::new(),
            address_deserializer: AddressDeserializer::new(),
        }
    }
}

impl Default for ThreadCycleStateDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<ThreadCycleState> for ThreadCycleStateDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ThreadCycleState, E> {
        context(
            "Failed ThreadCycleState deserialization",
            tuple((
                context("Failed cycle deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed last_final_slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context(
                    "Failed roll_count deserialization",
                    length_count(
                        |input| self.u32_deserializer.deserialize(input),
                        tuple((
                            |input| self.address_deserializer.deserialize(input),
                            |input| self.u64_deserializer.deserialize(input),
                        )),
                    )
                    .map(|res| RollCounts(res.into_iter().collect())),
                ),
                context(
                    "Failed cycle_updates deserialization",
                    length_count(
                        |input| self.u32_deserializer.deserialize(input),
                        tuple((
                            |input| self.address_deserializer.deserialize(input),
                            |input| self.roll_update_deserializer.deserialize(input),
                        )),
                    )
                    .map(|res| RollUpdates(res.into_iter().collect())),
                ),
                context("Failed rng_seed deserialization", |input| {
                    let (rest, n_entries) = self.u32_deserializer.deserialize(input)?;
                    let bits_u8_len = n_entries.div_ceil(u8::BITS) as usize;
                    if rest.len() < bits_u8_len {
                        return Err(nom::Err::Error(ParseError::from_error_kind(
                            input,
                            ErrorKind::Eof,
                        )));
                    }
                    let mut rng_seed: BitVec<u8> =
                        BitVec::try_from_vec(rest[..bits_u8_len].to_vec()).map_err(|_| {
                            nom::Err::Error(ParseError::from_error_kind(input, ErrorKind::Eof))
                        })?;
                    rng_seed.truncate(n_entries as usize);
                    if rng_seed.len() != n_entries as usize {
                        return Err(nom::Err::Error(ParseError::from_error_kind(
                            input,
                            ErrorKind::Eof,
                        )));
                    }
                    Ok((&rest[bits_u8_len..], rng_seed))
                }),
                context(
                    "Failed production_stats deserialization",
                    length_count(
                        |input| self.u32_deserializer.deserialize(input),
                        tuple((
                            |input| self.address_deserializer.deserialize(input),
                            |input| self.u64_deserializer.deserialize(input),
                            |input| self.u64_deserializer.deserialize(input),
                        )),
                    )
                    .map(|res| {
                        let mut production_stats = Map::default();
                        production_stats.extend(res.into_iter().map(
                            |(address, ok_count, nok_count)| (address, (ok_count, nok_count)),
                        ));
                        production_stats
                    }),
                ),
            )),
        )
        .map(
            |(cycle, last_final_slot, roll_count, cycle_updates, rng_seed, production_stats)| {
                ThreadCycleState {
                    cycle,
                    last_final_slot,
                    roll_count,
                    cycle_updates,
                    rng_seed,
                    production_stats,
                }
            },
        )
        .parse(buffer)
    }
}
