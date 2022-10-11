use bitvec::vec::BitVec;
use massa_models::{
    address::{Address, AddressDeserializer},
    prehash::PreHashMap,
};
use massa_serialization::{Deserializer, U64VarIntDeserializer};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use num::rational::Ratio;
use std::collections::BTreeMap;
use std::ops::Bound::Included;

/// State of a cycle for all threads
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct CycleInfo {
    /// cycle number
    pub cycle: u64,
    /// whether the cycle is complete (all slots final)
    pub complete: bool,
    /// number of rolls each staking address has
    pub roll_counts: BTreeMap<Address, u64>,
    /// random seed bits of all slots in the cycle so far
    pub rng_seed: BitVec<u8>,
    /// Per-address production statistics
    pub production_stats: PreHashMap<Address, ProductionStats>,
}

/// Block production statistic
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
pub struct ProductionStats {
    /// Number of successfully created blocks
    pub block_success_count: u64,
    /// Number of blocks missed
    pub block_failure_count: u64,
}

impl ProductionStats {
    /// Check if the production stats are above the required percentage
    pub fn is_satisfying(&self, max_miss_ratio: &Ratio<u64>) -> bool {
        let opportunities_count = self.block_success_count + self.block_failure_count;
        if opportunities_count == 0 {
            return true;
        }
        &Ratio::new(self.block_failure_count, opportunities_count) <= max_miss_ratio
    }

    /// Increment a production stat structure with another
    pub fn extend(&mut self, stats: &ProductionStats) {
        self.block_success_count = self
            .block_success_count
            .saturating_add(stats.block_success_count);
        self.block_failure_count = self
            .block_failure_count
            .saturating_add(stats.block_failure_count);
    }
}

/// Deserializer for ProductionStats
pub struct ProductionStatsDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl ProductionStatsDeserializer {
    /// Creates a new ProductionStats deserializer
    pub fn new() -> ProductionStatsDeserializer {
        ProductionStatsDeserializer {
            address_deserializer: AddressDeserializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<PreHashMap<Address, ProductionStats>> for ProductionStatsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PreHashMap<Address, ProductionStats>, E> {
        context(
            "Failed ProductionStats deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.address_deserializer.deserialize(input),
                    |input| self.u64_deserializer.deserialize(input),
                    |input| self.u64_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| {
            elements
                .into_iter()
                .map(|(addr, block_success_count, block_failure_count)| {
                    (
                        addr,
                        ProductionStats {
                            block_success_count,
                            block_failure_count,
                        },
                    )
                })
                .collect()
        })
        .parse(buffer)
    }
}

pub struct RollChangesDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl RollChangesDeserializer {
    pub fn new() -> RollChangesDeserializer {
        RollChangesDeserializer {
            address_deserializer: AddressDeserializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<PreHashMap<Address, u64>> for RollChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PreHashMap<Address, u64>, E> {
        context(
            "Failed RollChanges deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.address_deserializer.deserialize(input),
                    |input| self.u64_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
    }
}
