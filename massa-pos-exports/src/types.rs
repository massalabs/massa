// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{BTreeMap, VecDeque};

use bitvec::prelude::*;
use massa_models::{
    constants::{POS_MISS_RATE_DEACTIVATION_THRESHOLD, THREAD_COUNT},
    prehash::Map,
    Address, AddressDeserializer, Amount, AmountDeserializer, AmountSerializer, Slot,
    SlotDeserializer, SlotSerializer,
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use num::rational::Ratio;
use std::ops::Bound::Included;

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
    pub rng_seed: BitVec<Lsb0, u8>,
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
    pub seed_bits: BitVec<Lsb0, u8>,

    /// new roll counts for addresses (can be 0 to remove the address from the registry)
    pub roll_changes: Map<Address, u64>,

    /// updated production statistics
    pub production_stats: Map<Address, ProductionStats>,

    /// set deferred credits indexed by target slot (can be set to 0 to cancel some, in case of slash)
    /// ordered structure to ensure slot iteration order is deterministic
    pub deferred_credits: BTreeMap<Slot, Map<Address, Amount>>,
}

/// DOC TODO
/// AFTER LUNCH: IMPLEMENT NEWS AND POSCHANGES DESERIALIZER + UPDATE STATES_CHANGES SER / DESER
pub struct PoSChangesSerializer {
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
    /// DOC TODO
    pub fn new() -> PoSChangesSerializer {
        PoSChangesSerializer {
            u64_serializer: U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_serializer: SlotSerializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
            amount_serializer: AmountSerializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Serializer<PoSChanges> for PoSChangesSerializer {
    fn serialize(&self, value: &PoSChanges, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // roll_changes
        {
            let entry_count: u64 = value.roll_changes.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many entries in roll_changes: {}", err))
            })?;
            self.u64_serializer.serialize(&entry_count, buffer)?;
            for (addr, roll) in value.roll_changes.iter() {
                buffer.extend(addr.to_bytes());
                self.u64_serializer.serialize(roll, buffer)?;
            }
        };
        // production_stats
        {
            let entry_count: u64 = value.production_stats.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!(
                    "too many entries in production_stats: {}",
                    err
                ))
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
        };
        // deferred_credit
        {
            let entry_count: u64 = value.production_stats.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!(
                    "too many entries in deferred_credits: {}",
                    err
                ))
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
        };
        Ok(())
    }
}

struct RollChangesDeserializer {
    address_deserializer: AddressDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl Default for RollChangesDeserializer {
    fn default() -> Self {
        Self::new()
    }
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
                (Included(0), Included(THREAD_COUNT)),
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
