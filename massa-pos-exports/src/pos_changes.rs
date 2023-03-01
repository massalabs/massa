use crate::{
    DeferredCredits, DeferredCreditsDeserializer, DeferredCreditsSerializer, ProductionStats,
    ProductionStatsDeserializer, ProductionStatsSerializer, RollsDeserializer,
};
use bitvec::prelude::*;
use massa_models::{
    address::{Address, AddressSerializer},
    prehash::PreHashMap,
    serialization::{BitVecDeserializer, BitVecSerializer},
};
use massa_serialization::{Deserializer, SerializeError, Serializer, U64VarIntSerializer};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};

/// Recap of all PoS changes
#[derive(Default, Debug, Clone, Deserialize, Serialize)]
pub struct PoSChanges {
    /// extra block seed bits added
    pub seed_bits: BitVec<u8>,

    /// new roll counts for addresses (can be 0 to remove the address from the registry)
    pub roll_changes: PreHashMap<Address, u64>,

    /// updated production statistics
    pub production_stats: PreHashMap<Address, ProductionStats>,

    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: DeferredCredits,
}

impl PoSChanges {
    /// Check if changes are empty
    pub fn is_empty(&self) -> bool {
        self.seed_bits.is_empty()
            && self.roll_changes.is_empty()
            && self.production_stats.is_empty()
            && self.deferred_credits.credits.is_empty()
    }

    /// Extends the current `PosChanges` with another one
    pub fn extend(&mut self, other: PoSChanges) {
        // extend seed bits
        self.seed_bits.extend(other.seed_bits);

        // extend roll changes
        self.roll_changes.extend(other.roll_changes);

        // extend production stats
        for (other_addr, other_stats) in other.production_stats {
            self.production_stats
                .entry(other_addr)
                .or_insert_with(ProductionStats::default)
                .extend(&other_stats);
        }

        // extend deferred credits
        self.deferred_credits.nested_extend(other.deferred_credits);
    }
}

/// `PoSChanges` Serializer
pub struct PoSChangesSerializer {
    bit_vec_serializer: BitVecSerializer,
    u64_serializer: U64VarIntSerializer,
    production_stats_serializer: ProductionStatsSerializer,
    address_serializer: AddressSerializer,
    deferred_credits_serializer: DeferredCreditsSerializer,
}

impl Default for PoSChangesSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl PoSChangesSerializer {
    /// Create a new `PoSChanges` Serializer
    pub fn new() -> PoSChangesSerializer {
        PoSChangesSerializer {
            bit_vec_serializer: BitVecSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            production_stats_serializer: ProductionStatsSerializer::new(),
            address_serializer: AddressSerializer::new(),
            deferred_credits_serializer: DeferredCreditsSerializer::new(),
        }
    }
}

impl Serializer<PoSChanges> for PoSChangesSerializer {
    fn serialize(&self, value: &PoSChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // seed_bits
        self.bit_vec_serializer
            .serialize(&value.seed_bits, buffer)?;

        // roll_changes
        self.u64_serializer
            .serialize(&(value.roll_changes.len() as u64), buffer)?;
        for (addr, roll) in value.roll_changes.iter() {
            self.address_serializer.serialize(addr, buffer)?;
            self.u64_serializer.serialize(roll, buffer)?;
        }

        // production_stats
        self.production_stats_serializer
            .serialize(&value.production_stats, buffer)?;

        // deferred_credits
        self.deferred_credits_serializer
            .serialize(&value.deferred_credits, buffer)?;

        Ok(())
    }
}

/// `PoSChanges` Deserializer
pub struct PoSChangesDeserializer {
    bit_vec_deserializer: BitVecDeserializer,
    rolls_deserializer: RollsDeserializer,
    production_stats_deserializer: ProductionStatsDeserializer,
    deferred_credits_deserializer: DeferredCreditsDeserializer,
}

impl PoSChangesDeserializer {
    /// Create a new `PoSChanges` Deserializer
    pub fn new(
        thread_count: u8,
        max_rolls_length: u64,
        max_production_stats_length: u64,
        max_credits_length: u64,
    ) -> PoSChangesDeserializer {
        PoSChangesDeserializer {
            bit_vec_deserializer: BitVecDeserializer::new(),
            rolls_deserializer: RollsDeserializer::new(max_rolls_length),
            production_stats_deserializer: ProductionStatsDeserializer::new(
                max_production_stats_length,
            ),
            deferred_credits_deserializer: DeferredCreditsDeserializer::new(
                thread_count,
                max_credits_length,
            ),
        }
    }
}

impl Deserializer<PoSChanges> for PoSChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PoSChanges, E> {
        context(
            "Failed PoSChanges deserialization",
            tuple((
                context("Failed bit_vec deserialization", |input| {
                    self.bit_vec_deserializer.deserialize(input)
                }),
                context("Failed rolls deserialization", |input| {
                    self.rolls_deserializer.deserialize(input)
                }),
                context("Failed production_stats deserialization", |input| {
                    self.production_stats_deserializer.deserialize(input)
                }),
                context("Failed deferred_credits deserialization", |input| {
                    self.deferred_credits_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(seed_bits, roll_changes, production_stats, deferred_credits)| PoSChanges {
                seed_bits,
                roll_changes: roll_changes.into_iter().collect(),
                production_stats,
                deferred_credits,
            },
        )
        .parse(buffer)
    }
}
