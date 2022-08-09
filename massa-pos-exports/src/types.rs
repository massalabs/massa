// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{BTreeMap, VecDeque};

use bitvec::prelude::*;
use massa_models::{
    constants::{
        default::{CYCLE_INFO_SIZE_MESSAGE_BYTES, DEFERRED_CREDITS_PART_SIZE_MESSAGE_BYTES},
        POS_MISS_RATE_DEACTIVATION_THRESHOLD, THREAD_COUNT,
    },
    prehash::Map,
    Address, AddressDeserializer, Amount, AmountDeserializer, AmountSerializer, BitVecDeserializer,
    BitVecSerializer, ModelsError, Slot, SlotDeserializer, SlotSerializer,
};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::{length_count, many0},
    sequence::tuple,
    IResult, Parser,
};
use num::rational::Ratio;
use std::ops::Bound::{Excluded, Included, Unbounded};

use crate::SelectorController;

/// Final state of PoS
#[derive(Default)]
pub struct PoSFinalState {
    /// contiguous cycle history. Front = newest.
    pub cycle_history: VecDeque<CycleInfo>,
    /// coins to be credited at the end of the slot
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
    /// selector controller to feed the cycle when completed
    pub selector: Option<Box<dyn SelectorController>>,
}

/// Cursor object used for the Proof of Stake state bootstrap streaming
#[derive(Default)]
pub struct PoSBootstrapCursor {
    credits_slot: Option<Slot>,
    cycle: Option<u64>,
}

impl PoSFinalState {
    /// Private function used in `get_pos_state_part`
    fn get_cycles_part(
        &self,
        cursor: PoSBootstrapCursor,
    ) -> Result<(Vec<u8>, Option<u64>), ModelsError> {
        let last_cycle_index = if let Some(last_cycle) = cursor.cycle {
            if let Some(index) = self
                .cycle_history
                .iter()
                .position(|item| item.cycle == last_cycle)
            {
                Excluded(index)
            } else {
                Unbounded
            }
        } else if self.deferred_credits.first_key_value().is_some() {
            Unbounded
        } else {
            return Ok((Vec::new(), None));
        };
        let mut part = Vec::new();
        let mut last_cycle = None;
        let u64_ser = U64VarIntSerializer::new();
        let bitvec_ser = BitVecSerializer::new();
        for CycleInfo {
            cycle,
            complete,
            roll_counts,
            rng_seed,
            production_stats,
        } in self.cycle_history.range((last_cycle_index, Unbounded))
        {
            if part.len() < CYCLE_INFO_SIZE_MESSAGE_BYTES as usize {
                u64_ser.serialize(cycle, &mut part)?;
                // TODO: consider serializing this boolean some other way
                u64_ser.serialize(&(*complete as u64), &mut part)?;
                // TODO: limit this with ROLL_COUNTS_PART_SIZE_MESSAGE_BYTES
                for (addr, count) in roll_counts {
                    part.extend(addr.to_bytes());
                    u64_ser.serialize(&count, &mut part)?;
                }
                bitvec_ser.serialize(rng_seed, &mut part)?;
                // TODO: limit this with PRODUCTION_STATS_PART_SIZE_MESSAGE_BYTES
                for (
                    addr,
                    ProductionStats {
                        block_success_count,
                        block_failure_count,
                    },
                ) in production_stats
                {
                    part.extend(addr.to_bytes());
                    u64_ser.serialize(&block_success_count, &mut part)?;
                    u64_ser.serialize(&block_failure_count, &mut part)?;
                }
                last_cycle = Some(*cycle);
                // TODO: when roll_counts and production_stats are limited remove following break call
                break;
            }
        }
        Ok((part, last_cycle))
    }

    /// Gets a part of the Proof of Stake state. Used only in the bootstrap process.
    ///
    /// # Arguments:
    /// `cursor`: indicates the bootstrap state after the previous payload
    ///
    /// # Returns
    /// The PoS part and the updated cursor
    pub fn get_pos_state_part(
        &self,
        cursor: PoSBootstrapCursor,
    ) -> Result<(Vec<u8>, PoSBootstrapCursor), ModelsError> {
        let last_slot = if let Some(last_slot) = cursor.credits_slot {
            Excluded(last_slot)
        } else if self.deferred_credits.first_key_value().is_some() {
            Unbounded
        } else {
            let (part, last_cycle) = self.get_cycles_part(cursor)?;
            return Ok((
                part,
                PoSBootstrapCursor {
                    credits_slot: None,
                    cycle: last_cycle,
                },
            ));
        };
        let mut part = Vec::new();
        let mut last_credits_slot = None;
        let slot_ser = SlotSerializer::new();
        let amount_ser = AmountSerializer::new();
        for (slot, credits) in self.deferred_credits.range((last_slot, Unbounded)) {
            if part.len() < DEFERRED_CREDITS_PART_SIZE_MESSAGE_BYTES as usize {
                slot_ser.serialize(slot, &mut part)?;
                for (addr, amount) in credits {
                    part.extend(addr.to_bytes());
                    amount_ser.serialize(amount, &mut part)?;
                }
                last_credits_slot = Some(*slot);
            }
        }
        let (part, last_cycle) = self.get_cycles_part(cursor)?;
        Ok((
            part,
            PoSBootstrapCursor {
                credits_slot: last_credits_slot,
                cycle: last_cycle,
            },
        ))
    }

    /// Sets a part of the Proof of Stake state. Used only in the bootstrap process.
    ///
    /// # Arguments
    /// `part`: the raw data received from `get_pos_state_part` and used to update PoS State
    pub fn set_pos_state_part<'a>(&mut self, part: &'a [u8]) -> Result<(), ModelsError> {
        // TODO: define deserializers limits
        let amount_deser = AmountDeserializer::new(Included(u64::MIN), Included(u64::MAX));
        let slot_deser = SlotDeserializer::new(
            (Included(u64::MIN), Included(u64::MAX)),
            (Included(0), Excluded(THREAD_COUNT)),
        );
        let u64_deser = U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX));
        let bitvec_deser = BitVecDeserializer::new();
        let address_deser = AddressDeserializer::new();
        // NOTE: many0 instead of length_count because of the payload limit making it impossible to serialize the length
        let (rest, (credits, cycles)) = context(
            "Failed PoSFinalState deserialization",
            tuple((
                context(
                    "deferred_credits",
                    many0(tuple((
                        context("slot", |input| {
                            slot_deser.deserialize::<DeserializeError>(input)
                        }),
                        many0(tuple((
                            context("address", |input| address_deser.deserialize(input)),
                            context("amount", |input| amount_deser.deserialize(input)),
                        ))),
                    ))),
                ),
                context(
                    "cycle_history",
                    many0(tuple((
                        context("cycle", |input| u64_deser.deserialize(input)),
                        context("complete", |input| u64_deser.deserialize(input)),
                        context(
                            "roll_counts",
                            many0(tuple((
                                context("address", |input| address_deser.deserialize(input)),
                                context("count", |input| u64_deser.deserialize(input)),
                            ))),
                        ),
                        context("rng_seed", |input| bitvec_deser.deserialize(input)),
                        context(
                            "production_stats",
                            many0(tuple((
                                context("address", |input| address_deser.deserialize(input)),
                                context("block_success_count", |input| {
                                    u64_deser.deserialize(input)
                                }),
                                context("block_failure_count", |input| {
                                    u64_deser.deserialize(input)
                                }),
                            ))),
                        ),
                    ))),
                ),
            )),
        )
        .parse(part)
        .unwrap();
        // output type: (Vec<(Slot, Vec<(Address, Amount)>)>, Vec<(u64, u64, Vec<(Address, u64)>, bitvec::vec::BitVec<u8>, Vec<(Address, u64, u64)>)>)
        if rest.is_empty() {
            let sorted_credits: BTreeMap<Slot, Map<Address, Amount>> = credits
                .into_iter()
                .map(|(slot, credits)| (slot, credits.into_iter().collect()))
                .collect();
            self.deferred_credits.extend(sorted_credits);
            for item in cycles {
                let stats_iter =
                    item.4
                        .into_iter()
                        .map(|(addr, block_success_count, block_failure_count)| {
                            (
                                addr,
                                ProductionStats {
                                    block_success_count,
                                    block_failure_count,
                                },
                            )
                        });
                if let Some(info) = self.cycle_history.front_mut() && info.cycle == item.0 {
                    info.complete = if item.1 == 1 { true } else { false };
                    info.roll_counts.extend(item.2);
                    info.rng_seed.extend(item.3);
                    info.production_stats.extend(stats_iter);
                } else {
                    self.cycle_history.push_front(CycleInfo {
                        cycle: item.0,
                        complete: if item.1 == 1 { true } else { false },
                        roll_counts: item.2.into_iter().collect(),
                        rng_seed: item.3,
                        production_stats: stats_iter.collect(),
                    })
                }
            }
            Ok(())
        } else {
            Err(ModelsError::SerializeError(
                "data is left after PoSFinalState part deserialization".to_string(),
            ))
        }
    }
}

/// State of a cycle for all threads
#[derive(Default, Debug, Clone)]
pub struct CycleInfo {
    /// cycle number
    pub cycle: u64,
    /// whether the cycle is complete (all slots final)
    pub complete: bool,
    /// number of rolls each staking address has
    pub roll_counts: Map<Address, u64>,
    /// random seed bits of all slots in the cycle so far
    pub rng_seed: BitVec<u8>,
    /// Per-address production statistics
    pub production_stats: Map<Address, ProductionStats>,
}

/// Block production statistic
#[derive(Default, Debug, Copy, Clone)]
pub struct ProductionStats {
    /// Number of successfully created blocks
    pub block_success_count: u64,
    /// Number of blocks missed
    pub block_failure_count: u64,
}

impl ProductionStats {
    /// Check if the production stats are above the required percentage
    pub fn satisfying(&self) -> bool {
        let opportunities_count = self.block_success_count + self.block_failure_count;
        if opportunities_count == 0 {
            return true;
        }
        Ratio::new(self.block_failure_count, opportunities_count)
            <= *POS_MISS_RATE_DEACTIVATION_THRESHOLD
    }

    /// Increment a production stat struct with another
    pub fn chain(&mut self, stats: &ProductionStats) {
        self.block_success_count = self
            .block_success_count
            .saturating_add(stats.block_success_count);
        self.block_failure_count = self
            .block_failure_count
            .saturating_add(stats.block_failure_count);
    }
}

/// Recap of all PoS changes
#[derive(Default, Debug, Clone)]
pub struct PoSChanges {
    /// extra block seed bits added
    pub seed_bits: BitVec<u8>,

    /// new roll counts for addresses (can be 0 to remove the address from the registry)
    pub roll_changes: Map<Address, u64>,

    /// updated production statistics
    pub production_stats: Map<Address, ProductionStats>,

    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
}

impl PoSChanges {
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
                .or_insert_with(|| ProductionStats::default())
                .chain(&other_stats);
        }

        // extend deferred credits
        for (other_slot, other_credits) in other.deferred_credits {
            let self_credits = self
                .deferred_credits
                .entry(other_slot)
                .or_insert_with(|| Default::default());
            for (other_addr, other_amount) in other_credits {
                let self_amount = self_credits
                    .entry(other_addr)
                    .or_insert_with(|| Default::default());
                *self_amount = self_amount.saturating_add(other_amount);
            }
        }
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

        // deferred_credit
        let entry_count: u64 = value.production_stats.len().try_into().map_err(|err| {
            SerializeError::GeneralError(format!("too many entries in deferred_credits: {}", err))
        })?;
        self.u64_serializer.serialize(&entry_count, buffer)?;
        for (slot, credits) in value.deferred_credits.iter() {
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
    roll_changes_deserializer: RollChangesDeserializer,
    production_stats_deserializer: ProductionStatsDeserializer,
    deferred_credits_deserializer: DeferredCreditsDeserializer,
}

impl Default for PoSChangesDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl PoSChangesDeserializer {
    /// Create a new `PoSChanges` Deserializer
    pub fn new() -> PoSChangesDeserializer {
        PoSChangesDeserializer {
            bit_vec_deserializer: BitVecDeserializer::new(),
            roll_changes_deserializer: RollChangesDeserializer::new(),
            production_stats_deserializer: ProductionStatsDeserializer::new(),
            deferred_credits_deserializer: DeferredCreditsDeserializer::new(),
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

struct RollChangesDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl RollChangesDeserializer {
    fn new() -> RollChangesDeserializer {
        RollChangesDeserializer {
            address_deserializer: AddressDeserializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<Map<Address, u64>> for RollChangesDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Map<Address, u64>, E> {
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

struct ProductionStatsDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl ProductionStatsDeserializer {
    fn new() -> ProductionStatsDeserializer {
        ProductionStatsDeserializer {
            address_deserializer: AddressDeserializer::new(),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<Map<Address, ProductionStats>> for ProductionStatsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Map<Address, ProductionStats>, E> {
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

struct DeferredCreditsDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    slot_deserializer: SlotDeserializer,
    credit_deserializer: CreditDeserializer,
}

impl DeferredCreditsDeserializer {
    fn new() -> DeferredCreditsDeserializer {
        DeferredCreditsDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(THREAD_COUNT)),
            ),
            credit_deserializer: CreditDeserializer::new(),
        }
    }
}

impl Deserializer<BTreeMap<Slot, Map<Address, Amount>>> for DeferredCreditsDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BTreeMap<Slot, Map<Address, Amount>>, E> {
        context(
            "Failed DeferredCredits deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.slot_deserializer.deserialize(input),
                    |input| self.credit_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
    }
}

struct CreditDeserializer {
    u64_deserializer: U64VarIntDeserializer,
    address_deserializer: AddressDeserializer,
    amount_deserializer: AmountDeserializer,
}

impl CreditDeserializer {
    fn new() -> CreditDeserializer {
        CreditDeserializer {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            address_deserializer: AddressDeserializer::new(),
            amount_deserializer: AmountDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<Map<Address, Amount>> for CreditDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Map<Address, Amount>, E> {
        context(
            "Failed Credit deserialization",
            length_count(
                context("Failed length deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                tuple((
                    |input| self.address_deserializer.deserialize(input),
                    |input| self.amount_deserializer.deserialize(input),
                )),
            ),
        )
        .map(|elements| elements.into_iter().collect())
        .parse(buffer)
    }
}

/// Selections of endorsements and producer
#[derive(Clone)]
pub struct Selection {
    /// Choosen endorsements
    pub endorsments: Vec<Address>,
    /// Choosen block producer
    pub producer: Address,
}
