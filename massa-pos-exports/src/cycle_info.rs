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
    production_stats_ser: ProductionStatsSerializer,
}

impl Default for CycleInfoSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl CycleInfoSerializer {
    /// Creates a new `CycleInfo` serializer
    pub fn new() -> Self {
        Self {
            u64_ser: U64VarIntSerializer::new(),
            bitvec_ser: BitVecSerializer::new(),
            production_stats_ser: ProductionStatsSerializer::new(),
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
        self.production_stats_ser
            .serialize(&value.production_stats, buffer)?;

        Ok(())
    }
}

/// Deserializer for `CycleInfo`
pub struct CycleInfoDeserializer {
    u64_deser: U64VarIntDeserializer,
    rolls_deser: RollsDeserializer,
    bitvec_deser: BitVecDeserializer,
    production_stats_deser: ProductionStatsDeserializer,
}

impl CycleInfoDeserializer {
    /// Creates a new `CycleInfo` deserializer
    pub fn new(max_rolls_length: u64, max_production_stats_length: u64) -> CycleInfoDeserializer {
        CycleInfoDeserializer {
            u64_deser: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            rolls_deser: RollsDeserializer::new(max_rolls_length),
            bitvec_deser: BitVecDeserializer::new(),
            production_stats_deser: ProductionStatsDeserializer::new(max_production_stats_length),
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
                context("roll_counts", |input| self.rolls_deser.deserialize(input)),
                context("rng_seed", |input| self.bitvec_deser.deserialize(input)),
                context("production_stats", |input| {
                    self.production_stats_deser.deserialize(input)
                }),
            )),
        )
        .map(
            #[allow(clippy::type_complexity)]
            |(cycle, complete, roll_counts, rng_seed, production_stats): (
                u64,                                  // cycle
                bool,                                 // complete
                Vec<(Address, u64)>,                  // roll_counts
                BitVec<u8>,                           // rng_seed
                PreHashMap<Address, ProductionStats>, // production_stats (address, n_success, n_fail)
            )| CycleInfo {
                cycle,
                complete,
                roll_counts: roll_counts.into_iter().collect(),
                rng_seed,
                production_stats,
            },
        )
        .parse(buffer)
    }
}

/// Block production statistics
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

/// Serializer for `ProductionStats`
pub struct ProductionStatsSerializer {
    u64_ser: U64VarIntSerializer,
}

impl Default for ProductionStatsSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl ProductionStatsSerializer {
    /// Creates a new `ProductionStats` serializer
    pub fn new() -> Self {
        Self {
            u64_ser: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<PreHashMap<Address, ProductionStats>> for ProductionStatsSerializer {
    fn serialize(
        &self,
        value: &PreHashMap<Address, ProductionStats>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_ser.serialize(&(value.len() as u64), buffer)?;
        for (
            addr,
            ProductionStats {
                block_success_count,
                block_failure_count,
            },
        ) in value.iter()
        {
            buffer.extend(addr.to_bytes());
            self.u64_ser.serialize(block_success_count, buffer)?;
            self.u64_ser.serialize(block_failure_count, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `ProductionStats`
pub struct ProductionStatsDeserializer {
    length_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl ProductionStatsDeserializer {
    /// Creates a new `ProductionStats` deserializer
    pub fn new(max_production_stats_length: u64) -> ProductionStatsDeserializer {
        ProductionStatsDeserializer {
            length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_production_stats_length),
            ),
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
                    self.length_deserializer.deserialize(input)
                }),
                tuple((
                    context("Failed address deserialization", |input| {
                        self.address_deserializer.deserialize(input)
                    }),
                    context("Failed block_success_count deserialization", |input| {
                        self.u64_deserializer.deserialize(input)
                    }),
                    context("Failed block_failure_count deserialization", |input| {
                        self.u64_deserializer.deserialize(input)
                    }),
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
    length_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl RollsDeserializer {
    /// Creates a new rolls deserializer
    pub fn new(max_rolls_length: u64) -> RollsDeserializer {
        RollsDeserializer {
            length_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_rolls_length),
            ),
            address_deserializer: AddressDeserializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<Vec<(Address, u64)>> for RollsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Vec<(Address, u64)>, E> {
        context(
            "Failed rolls deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.length_deserializer.deserialize(input)
                }),
                tuple((
                    context("Failed address deserialization", |input| {
                        self.address_deserializer.deserialize(input)
                    }),
                    context("Failed number deserialization", |input| {
                        self.u64_deserializer.deserialize(input)
                    }),
                )),
            ),
        )
        .parse(buffer)
    }
}
