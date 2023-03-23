use bitvec::vec::BitVec;
use massa_hash::{Hash, HashDeserializer, HashSerializer, HASH_SIZE_BYTES};
use massa_models::{
    address::{Address, AddressDeserializer, AddressSerializer},
    prehash::PreHashMap,
    serialization::{BitVecDeserializer, BitVecSerializer},
    slot::Slot,
};
use massa_serialization::{
    Deserializer, OptionDeserializer, OptionSerializer, SerializeError, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
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
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::ops::Bound::Included;
use tracing::info;

use crate::PoSChanges;

const CYCLE_INFO_HASH_INITIAL_BYTES: &[u8; 32] = &[0; HASH_SIZE_BYTES];

struct CycleInfoHashComputer {
    u64_ser: U64VarIntSerializer,
    address_ser: AddressSerializer,
    bitvec_ser: BitVecSerializer,
}

impl CycleInfoHashComputer {
    fn new() -> Self {
        Self {
            u64_ser: U64VarIntSerializer::new(),
            address_ser: AddressSerializer::new(),
            bitvec_ser: BitVecSerializer::new(),
        }
    }

    fn compute_cycle_hash(&self, cycle: u64) -> Hash {
        // serialization can never fail in the following computations, unwrap is justified
        let mut buffer = Vec::new();
        self.u64_ser.serialize(&cycle, &mut buffer).unwrap();
        Hash::compute_from(&buffer)
    }

    fn compute_complete_hash(&self, complete: bool) -> Hash {
        let mut buffer = Vec::new();
        self.u64_ser
            .serialize(&(complete as u64), &mut buffer)
            .unwrap();
        Hash::compute_from(&buffer)
    }

    fn compute_seed_hash(&self, seed: &BitVec<u8>) -> Hash {
        let mut buffer = Vec::new();
        self.bitvec_ser.serialize(seed, &mut buffer).unwrap();
        Hash::compute_from(&buffer)
    }

    fn compute_roll_entry_hash(&self, address: &Address, roll_count: u64) -> Hash {
        let mut buffer = Vec::new();
        self.address_ser.serialize(address, &mut buffer).unwrap();
        self.u64_ser.serialize(&roll_count, &mut buffer).unwrap();
        Hash::compute_from(&buffer)
    }

    fn compute_prod_stats_entry_hash(
        &self,
        address: &Address,
        prod_stats: &ProductionStats,
    ) -> Hash {
        let mut buffer = Vec::new();
        self.address_ser.serialize(address, &mut buffer).unwrap();
        self.u64_ser
            .serialize(&prod_stats.block_success_count, &mut buffer)
            .unwrap();
        self.u64_ser
            .serialize(&prod_stats.block_failure_count, &mut buffer)
            .unwrap();
        Hash::compute_from(&buffer)
    }
}

/// State of a cycle for all threads
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Hash of the roll counts
    pub roll_counts_hash: Hash,
    /// Hash of the production statistics
    pub production_stats_hash: Hash,
    /// Hash of the cycle state
    pub cycle_global_hash: Hash,
    /// Snapshot of the final state hash
    /// Used for PoS selections
    pub final_state_hash_snapshot: Option<Hash>,
}

impl CycleInfo {
    /// Create a new `CycleInfo` and compute its hash
    pub fn new_with_hash(
        cycle: u64,
        complete: bool,
        roll_counts: BTreeMap<Address, u64>,
        rng_seed: BitVec<u8>,
        production_stats: PreHashMap<Address, ProductionStats>,
    ) -> Self {
        let hash_computer = CycleInfoHashComputer::new();
        let mut roll_counts_hash = Hash::from_bytes(CYCLE_INFO_HASH_INITIAL_BYTES);
        let mut production_stats_hash = Hash::from_bytes(CYCLE_INFO_HASH_INITIAL_BYTES);

        // compute the cycle hash
        let mut hash_concat: Vec<u8> = Vec::new();
        hash_concat.extend(hash_computer.compute_cycle_hash(cycle).to_bytes());
        hash_concat.extend(hash_computer.compute_complete_hash(complete).to_bytes());
        hash_concat.extend(hash_computer.compute_seed_hash(&rng_seed).to_bytes());
        for (addr, &count) in &roll_counts {
            roll_counts_hash ^= hash_computer.compute_roll_entry_hash(addr, count);
        }
        hash_concat.extend(roll_counts_hash.to_bytes());
        for (addr, prod_stats) in &production_stats {
            production_stats_hash ^= hash_computer.compute_prod_stats_entry_hash(addr, prod_stats);
        }
        hash_concat.extend(production_stats_hash.to_bytes());

        // compute the global hash
        let cycle_global_hash = Hash::compute_from(&hash_concat);

        // create the new cycle
        CycleInfo {
            cycle,
            complete,
            roll_counts,
            rng_seed,
            production_stats,
            roll_counts_hash,
            production_stats_hash,
            cycle_global_hash,
            final_state_hash_snapshot: None,
        }
    }

    /// Apply every part of a `PoSChanges` to a cycle info, except for `deferred_credits`
    pub(crate) fn apply_changes(
        &mut self,
        changes: PoSChanges,
        slot: Slot,
        periods_per_cycle: u64,
        thread_count: u8,
    ) -> bool {
        info!("CYCLE INFO CHANGES : {:?}", changes);

        let hash_computer = CycleInfoHashComputer::new();
        let slots_per_cycle = periods_per_cycle.saturating_mul(thread_count as u64);
        let mut hash_concat: Vec<u8> = Vec::new();

        // compute cycle hash and concat
        let cycle_hash = hash_computer.compute_cycle_hash(self.cycle);
        hash_concat.extend(cycle_hash.to_bytes());

        // check for completion
        self.complete = slot.is_last_of_cycle(periods_per_cycle, thread_count);
        let complete_hash = hash_computer.compute_complete_hash(self.complete);
        hash_concat.extend(complete_hash.to_bytes());

        // extend seed_bits with changes.seed_bits
        self.rng_seed.extend(changes.seed_bits);
        let rng_seed_hash = hash_computer.compute_seed_hash(&self.rng_seed);
        hash_concat.extend(rng_seed_hash.to_bytes());

        // extend roll counts
        for (addr, roll_count) in changes.roll_changes {
            if roll_count == 0 && let Some(removed_count) = self.roll_counts.remove(&addr) {
                self.roll_counts_hash ^=
                    hash_computer.compute_roll_entry_hash(&addr, removed_count);
            } else {
                if let Some(replaced_count) = self.roll_counts.insert(addr, roll_count) {
                    self.roll_counts_hash ^=
                        hash_computer.compute_roll_entry_hash(&addr, replaced_count);
                }
                self.roll_counts_hash ^= hash_computer.compute_roll_entry_hash(&addr, roll_count);
            }
        }
        hash_concat.extend(self.roll_counts_hash.to_bytes());

        // extend production stats
        for (addr, stats) in changes.production_stats {
            self.production_stats
                .entry(addr)
                .and_modify(|current_stats| {
                    self.production_stats_hash ^=
                        hash_computer.compute_prod_stats_entry_hash(&addr, current_stats);
                    current_stats.extend(&stats);
                    self.production_stats_hash ^=
                        hash_computer.compute_prod_stats_entry_hash(&addr, current_stats);
                })
                .or_insert_with(|| {
                    self.production_stats_hash ^=
                        hash_computer.compute_prod_stats_entry_hash(&addr, &stats);
                    stats
                });
        }
        hash_concat.extend(self.production_stats_hash.to_bytes());

        // if the cycle just completed, check that it has the right number of seed bits
        if self.complete && self.rng_seed.len() as u64 != slots_per_cycle {
            panic!(
                "cycle completed with incorrect number of seed bits: {} instead of {}",
                self.rng_seed.len(),
                slots_per_cycle
            );
        }

        // compute the global hash
        self.cycle_global_hash = Hash::compute_from(&hash_concat);

        // return the completion status
        self.complete
    }
}

#[test]
fn test_cycle_info_hash_computation() {
    use crate::DeferredCredits;
    use bitvec::prelude::*;

    // cycle and address
    let mut cycle_a = CycleInfo::new_with_hash(
        0,
        false,
        BTreeMap::default(),
        BitVec::default(),
        PreHashMap::default(),
    );
    let addr = Address::from_prefixed_bytes(&[0; 33].as_slice()).unwrap();

    // add changes
    let mut roll_changes = PreHashMap::default();
    roll_changes.insert(addr, 10);
    let mut production_stats = PreHashMap::default();
    production_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 4,
            block_failure_count: 0,
        },
    );
    let changes = PoSChanges {
        seed_bits: bitvec![u8, Lsb0; 0, 10],
        roll_changes: roll_changes.clone(),
        production_stats: production_stats.clone(),
        deferred_credits: DeferredCredits::default(),
    };
    cycle_a.apply_changes(changes, Slot::new(0, 0), 2, 2);

    // update changes once
    roll_changes.clear();
    roll_changes.insert(addr, 20);
    production_stats.clear();
    production_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 4,
            block_failure_count: 6,
        },
    );
    let changes = PoSChanges {
        seed_bits: bitvec![u8, Lsb0; 0, 20],
        roll_changes: roll_changes.clone(),
        production_stats: production_stats.clone(),
        deferred_credits: DeferredCredits::default(),
    };
    cycle_a.apply_changes(changes, Slot::new(0, 1), 2, 2);

    // update changes twice
    roll_changes.clear();
    roll_changes.insert(addr, 0);
    production_stats.clear();
    production_stats.insert(
        addr,
        ProductionStats {
            block_success_count: 4,
            block_failure_count: 12,
        },
    );
    let changes = PoSChanges {
        seed_bits: bitvec![u8, Lsb0; 0, 30],
        roll_changes,
        production_stats,
        deferred_credits: DeferredCredits::default(),
    };
    cycle_a.apply_changes(changes, Slot::new(1, 0), 2, 2);

    // create a seconde cycle from same value and match hash
    let cycle_b = CycleInfo::new_with_hash(
        0,
        cycle_a.complete,
        cycle_a.roll_counts,
        cycle_a.rng_seed,
        cycle_a.production_stats,
    );
    assert_eq!(
        cycle_a.roll_counts_hash, cycle_b.roll_counts_hash,
        "roll_counts_hash mismatch"
    );
    assert_eq!(
        cycle_a.production_stats_hash, cycle_b.production_stats_hash,
        "production_stats_hash mismatch"
    );
    assert_eq!(
        cycle_a.cycle_global_hash, cycle_b.cycle_global_hash,
        "global_hash mismatch"
    );
}

/// Serializer for `CycleInfo`
pub struct CycleInfoSerializer {
    u64_ser: U64VarIntSerializer,
    bitvec_ser: BitVecSerializer,
    production_stats_ser: ProductionStatsSerializer,
    address_ser: AddressSerializer,
    opt_hash_ser: OptionSerializer<Hash, HashSerializer>,
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
            address_ser: AddressSerializer::new(),
            opt_hash_ser: OptionSerializer::new(HashSerializer::new()),
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
            self.address_ser.serialize(addr, buffer)?;
            self.u64_ser.serialize(count, buffer)?;
        }

        // cycle_info.rng_seed
        self.bitvec_ser.serialize(&value.rng_seed, buffer)?;

        // cycle_info.production_stats
        self.production_stats_ser
            .serialize(&value.production_stats, buffer)?;

        // cycle_info.final_state_hash_snapshot
        self.opt_hash_ser
            .serialize(&value.final_state_hash_snapshot, buffer)?;

        Ok(())
    }
}

/// Deserializer for `CycleInfo`
pub struct CycleInfoDeserializer {
    u64_deser: U64VarIntDeserializer,
    rolls_deser: RollsDeserializer,
    bitvec_deser: BitVecDeserializer,
    production_stats_deser: ProductionStatsDeserializer,
    opt_hash_deser: OptionDeserializer<Hash, HashDeserializer>,
}

impl CycleInfoDeserializer {
    /// Creates a new `CycleInfo` deserializer
    pub fn new(max_rolls_length: u64, max_production_stats_length: u64) -> CycleInfoDeserializer {
        CycleInfoDeserializer {
            u64_deser: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            rolls_deser: RollsDeserializer::new(max_rolls_length),
            bitvec_deser: BitVecDeserializer::new(),
            production_stats_deser: ProductionStatsDeserializer::new(max_production_stats_length),
            opt_hash_deser: OptionDeserializer::new(HashDeserializer::new()),
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
                context("final_state_hash_snapshot", |input| {
                    self.opt_hash_deser.deserialize(input)
                }),
            )),
        )
        .map(
            #[allow(clippy::type_complexity)]
            |(cycle, complete, roll_counts, rng_seed, production_stats, opt_hash): (
                u64,                                  // cycle
                bool,                                 // complete
                Vec<(Address, u64)>,                  // roll_counts
                BitVec<u8>,                           // rng_seed
                PreHashMap<Address, ProductionStats>, // production_stats (address, n_success, n_fail)
                Option<Hash>,                         // final_state_hash_snapshot
            )| {
                let mut cycle = CycleInfo::new_with_hash(
                    cycle,
                    complete,
                    roll_counts.into_iter().collect(),
                    rng_seed,
                    production_stats,
                );
                cycle.final_state_hash_snapshot = opt_hash;
                cycle
            },
        )
        .parse(buffer)
    }
}

/// Block production statistics
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
    address_ser: AddressSerializer,
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
            address_ser: AddressSerializer::new(),
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
            self.address_ser.serialize(addr, buffer)?;
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

/// Serializer for cycle history
pub struct CycleHistorySerializer {
    u64_serializer: U64VarIntSerializer,
    cycle_info_serializer: CycleInfoSerializer,
}

impl CycleHistorySerializer {
    /// Creates a new `CycleHistory` serializer
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer::new(),
            cycle_info_serializer: CycleInfoSerializer::new(),
        }
    }
}

impl Default for CycleHistorySerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<VecDeque<CycleInfo>> for CycleHistorySerializer {
    fn serialize(
        &self,
        value: &VecDeque<CycleInfo>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.u64_serializer
            .serialize(&(value.len() as u64), buffer)?;
        for cycle_info in value.iter() {
            self.cycle_info_serializer.serialize(cycle_info, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for cycle history
pub struct CycleHistoryDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    cycle_info_deserializer: CycleInfoDeserializer,
}

impl CycleHistoryDeserializer {
    /// Creates a new `CycleHistory` deserializer
    pub fn new(
        max_cycle_history_length: u64,
        max_rolls_length: u64,
        max_production_stats_length: u64,
    ) -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(max_cycle_history_length),
            ),
            cycle_info_deserializer: CycleInfoDeserializer::new(
                max_rolls_length,
                max_production_stats_length,
            ),
        }
    }
}

impl Deserializer<Vec<CycleInfo>> for CycleHistoryDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Vec<CycleInfo>, E> {
        context(
            "Failed cycle_history deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed cycle_info deserialization", |input| {
                    self.cycle_info_deserializer.deserialize(input)
                }),
            ),
        )
        .parse(buffer)
    }
}
