use bitvec::vec::BitVec;
use massa_models::{
    address::{Address, AddressDeserializer},
    prehash::PreHashMap,
    serialization::{BitVecDeserializer, BitVecSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    branch::alt,
    bytes::complete::tag,
    combinator::value,
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

/// Serializer for `CycleInfo`
pub struct CycleInfoSerializer {
    u64_ser: U64VarIntSerializer,
    bitvec_ser: BitVecSerializer,
}

impl CycleInfoSerializer {
    /// Creates a new `CycleInfo` serializer
    pub fn new() -> Self {
        Self {
            u64_ser: U64VarIntSerializer::new(),
            bitvec_ser: BitVecSerializer::new(),
        }
    }
}

impl Serializer<CycleInfo> for CycleInfoSerializer {
    fn serialize(&self, value: &CycleInfo, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // cycle_info.cycle
        self.u64_ser.serialize(&value.cycle, buffer)?;

        // cycle_info.complete
        buffer.push(u8::from(value.complete));

        // cycle_info.roll_counts
        self.u64_ser
            .serialize(&(value.roll_counts.len() as u64), buffer)?;
        for (addr, count) in &value.roll_counts {
            buffer.extend(addr.to_bytes());
            self.u64_ser.serialize(count, buffer)?;
        }

        // cycle_info.rng_seed
        self.bitvec_ser.serialize(&value.rng_seed, buffer)?;

        // cycle_info.production_stats
        self.u64_ser
            .serialize(&(value.production_stats.len() as u64), buffer)?;
        for (addr, stats) in &value.production_stats {
            buffer.extend(addr.to_bytes());
            self.u64_ser.serialize(&stats.block_success_count, buffer)?;
            self.u64_ser.serialize(&stats.block_failure_count, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `CycleInfo`
pub struct CycleInfoDeserializer {
    address_deser: AddressDeserializer,
    u64_deser: U64VarIntDeserializer,
    bitvec_deser: BitVecDeserializer,
}

impl CycleInfoDeserializer {
    /// Creates a new `CycleInfo` deserializer
    pub fn new() -> CycleInfoDeserializer {
        CycleInfoDeserializer {
            address_deser: AddressDeserializer::new(),
            u64_deser: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            bitvec_deser: BitVecDeserializer::new(),
        }
    }
}

impl Deserializer<CycleInfo> for CycleInfoDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], CycleInfo, E> {
        context(
            "cycle_history",
            tuple((
                context("cycle", |input| self.u64_deser.deserialize(input)),
                context(
                    "complete",
                    alt((value(true, tag(&[1])), value(false, tag(&[0])))),
                ),
                context(
                    "roll_counts",
                    length_count(
                        context("roll_counts length", |input| {
                            self.u64_deser.deserialize(input)
                        }),
                        tuple((
                            context("address", |input| self.address_deser.deserialize(input)),
                            context("count", |input| self.u64_deser.deserialize(input)),
                        )),
                    ),
                ),
                context("rng_seed", |input| self.bitvec_deser.deserialize(input)),
                context(
                    "production_stats",
                    length_count(
                        context("production_stats length", |input| {
                            self.u64_deser.deserialize(input)
                        }),
                        tuple((
                            context("address", |input| self.address_deser.deserialize(input)),
                            context("block_success_count", |input| {
                                self.u64_deser.deserialize(input)
                            }),
                            context("block_failure_count", |input| {
                                self.u64_deser.deserialize(input)
                            }),
                        )),
                    ),
                ),
            )),
        )
        .map(
            |(cycle, complete, roll_counts, rng_seed, production_stats): (
                u64,                      // cycle
                bool,                     // complete
                Vec<(Address, u64)>,      // roll_counts
                bitvec::vec::BitVec<u8>,  // rng_seed
                Vec<(Address, u64, u64)>, // production_stats (address, n_success, n_fail)
            )| CycleInfo {
                cycle,
                complete,
                roll_counts: roll_counts,
                rng_seed,
                production_stats: production_stats,
            },
        )
        .parse(buffer)
    }
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

/// Deserializer for `ProductionStats`
pub struct ProductionStatsDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl ProductionStatsDeserializer {
    /// Creates a new `ProductionStats` deserializer
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

/// Deserializer for rolls
pub struct RollsDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl RollsDeserializer {
    /// Creates a new rolls deserializer
    pub fn new() -> RollsDeserializer {
        RollsDeserializer {
            address_deserializer: AddressDeserializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<PreHashMap<Address, u64>> for RollsDeserializer {
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
