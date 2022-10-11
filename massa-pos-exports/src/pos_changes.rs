use crate::{
    DeferredCredits, DeferredCreditsDeserializer, ProductionStats, ProductionStatsDeserializer,
    RollsDeserializer,
};
use bitvec::prelude::*;
use massa_models::{
    address::Address,
    amount::AmountSerializer,
    prehash::PreHashMap,
    serialization::{BitVecDeserializer, BitVecSerializer},
    slot::SlotSerializer,
};
use massa_serialization::{Deserializer, SerializeError, Serializer, U64VarIntSerializer};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};

/// Recap of all PoS changes
#[derive(Default, Debug, Clone)]
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
            && self.deferred_credits.0.is_empty()
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
    slot_serializer: SlotSerializer,
    amount_serializer: AmountSerializer,
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
            slot_serializer: SlotSerializer::new(),
            amount_serializer: AmountSerializer::new(),
        }
    }
}

impl Serializer<PoSChanges> for PoSChangesSerializer {
    fn serialize(&self, value: &PoSChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // seed_bits
        self.bit_vec_serializer
            .serialize(&value.seed_bits, buffer)?;

        // roll_changes
        let entry_count: u64 = value.roll_changes.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in roll_changes: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (addr, roll) in value.roll_changes.iter() {
            buffer.extend(addr.to_bytes());
            self.u64_serializer.serialize(roll, buffer)?;
        }

        // production_stats
        let entry_count: u64 = value.production_stats.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in production_stats: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (
            addr,
            ProductionStats {
                block_success_count,
                block_failure_count,
            },
        ) in value.production_stats.iter()
        {
            buffer.extend(addr.to_bytes());
            self.u64_serializer.serialize(block_success_count, buffer)?;
            self.u64_serializer.serialize(block_failure_count, buffer)?;
        }

        // deferred_credits
        let entry_count: u64 = value.deferred_credits.0.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in deferred_credits: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (slot, credits) in value.deferred_credits.0.iter() {
            self.slot_serializer.serialize(slot, buffer)?;
            let credits_entry_count: u64 = credits.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many entries in credits: {}", err))
            })?;
            self.u64_serializer
                .serialize(&credits_entry_count, buffer)?;
            for (addr, amount) in credits {
                buffer.extend(addr.to_bytes());
                self.amount_serializer.serialize(amount, buffer)?;
            }
        }
        Ok(())
    }
}

/// `PoSChanges` Deserializer
pub struct PoSChangesDeserializer {
    bit_vec_deserializer: BitVecDeserializer,
    roll_changes_deserializer: RollsDeserializer,
    production_stats_deserializer: ProductionStatsDeserializer,
    deferred_credits_deserializer: DeferredCreditsDeserializer,
}

impl PoSChangesDeserializer {
    /// Create a new `PoSChanges` Deserializer
    pub fn new(thread_count: u8) -> PoSChangesDeserializer {
        PoSChangesDeserializer {
            bit_vec_deserializer: BitVecDeserializer::new(),
            roll_changes_deserializer: RollsDeserializer::new(),
            production_stats_deserializer: ProductionStatsDeserializer::new(),
            deferred_credits_deserializer: DeferredCreditsDeserializer::new(thread_count),
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
                |input| self.bit_vec_deserializer.deserialize(input),
                |input| self.roll_changes_deserializer.deserialize(input),
                |input| self.production_stats_deserializer.deserialize(input),
                |input| self.deferred_credits_deserializer.deserialize(input),
            )),
        )
        .map(
            |(seed_bits, roll_changes, production_stats, deferred_credits)| PoSChanges {
                seed_bits,
                roll_changes,
                production_stats,
                deferred_credits,
            },
        )
        .parse(buffer)
    }
}
