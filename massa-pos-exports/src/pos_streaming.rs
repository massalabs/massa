// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::{CycleInfo, DeferredCredits, ProductionStats, SelectorController};
use massa_hash::Hash;
use massa_models::{
    address::{Address, AddressDeserializer},
    amount::{Amount, AmountDeserializer},
    error::ModelsError,
    serialization::BitVecDeserializer,
    slot::{Slot, SlotDeserializer},
};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
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
use std::collections::{BTreeMap, VecDeque};
use std::ops::Bound::{Excluded, Included, Unbounded};
use tracing::warn;

/// Final state of PoS
pub struct PoSFinalState {
    /// contiguous cycle history. Back = newest.
    pub cycle_history: VecDeque<CycleInfo>,
    /// coins to be credited at the end of the slot
    pub deferred_credits: DeferredCredits,
    /// selector controller
    pub selector: Box<dyn SelectorController>,
    /// initial rolls, used for negative cycle look back
    pub initial_rolls: BTreeMap<Address, u64>,
    /// initial seeds, used for negative cycle look back (cycles -2, -1 in that order)
    pub initial_seeds: Vec<Hash>,
    /// amount deserializer
    pub amount_deserializer: AmountDeserializer,
    /// slot deserializer
    pub slot_deserializer: SlotDeserializer,
    /// deserializer
    pub deferred_credit_length_deserializer: U64VarIntDeserializer,
    /// address deserializer
    pub address_deserializer: AddressDeserializer,
    /// periods per cycle
    pub periods_per_cycle: u64,
    /// thread count
    pub thread_count: u8,
}

/// PoS bootstrap streaming steps
#[derive(PartialEq, Eq, Copy, Clone, Debug)]
pub enum PoSCycleStreamingStep {
    /// Started step, only when launching the streaming
    Started,
    /// Ongoing step, as long as you are streaming complete cycles
    Ongoing(u64),
    /// Finished step, after the incomplete cycle was streamed
    Finished,
}

/// PoS bootstrap streaming steps serializer
#[derive(Default)]
pub struct PoSCycleStreamingStepSerializer {
    u64_serializer: U64VarIntSerializer,
}

impl PoSCycleStreamingStepSerializer {
    /// Creates a new PoS bootstrap streaming steps serializer
    pub fn new() -> Self {
        Self {
            u64_serializer: U64VarIntSerializer,
        }
    }
}

impl Serializer<PoSCycleStreamingStep> for PoSCycleStreamingStepSerializer {
    fn serialize(
        &self,
        value: &PoSCycleStreamingStep,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            PoSCycleStreamingStep::Started => self.u64_serializer.serialize(&0u64, buffer)?,
            PoSCycleStreamingStep::Ongoing(last_cycle) => {
                self.u64_serializer.serialize(&1u64, buffer)?;
                self.u64_serializer.serialize(last_cycle, buffer)?;
            }
            PoSCycleStreamingStep::Finished => self.u64_serializer.serialize(&2u64, buffer)?,
        };
        Ok(())
    }
}

/// PoS bootstrap streaming steps deserializer
pub struct PoSCycleStreamingStepDeserializer {
    u64_deserializer: U64VarIntDeserializer,
}

impl Default for PoSCycleStreamingStepDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl PoSCycleStreamingStepDeserializer {
    /// Creates a new PoS bootstrap streaming steps deserializer
    pub fn new() -> Self {
        Self {
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<PoSCycleStreamingStep> for PoSCycleStreamingStepDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], PoSCycleStreamingStep, E> {
        let (rest, ident) = context("identifier", |input| {
            self.u64_deserializer.deserialize(input)
        })
        .parse(buffer)?;
        match ident {
            0u64 => Ok((rest, PoSCycleStreamingStep::Started)),
            1u64 => context("cycle", |input| self.u64_deserializer.deserialize(input))
                .map(PoSCycleStreamingStep::Ongoing)
                .parse(rest),

            2u64 => Ok((rest, PoSCycleStreamingStep::Finished)),
            _ => Err(nom::Err::Error(ParseError::from_error_kind(
                buffer,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}

impl PoSFinalState {
    fn get_first_cycle_index(&self) -> usize {
        // for bootstrap:
        // if cycle_history is full skip the bootstrap safety cycle
        // if not stream it
        //
        // TODO: use config
        usize::from(self.cycle_history.len() >= 6)
    }

    /// Gets a cycle of the Proof of Stake `cycle_history`. Used only in the bootstrap process.
    ///
    /// # Arguments:
    /// `cursor`: indicates the bootstrap state after the previous payload
    ///
    /// # Returns
    /// The PoS cycle and the updated cursor
    pub fn get_cycle_history_part(
        &self,
        cursor: PoSCycleStreamingStep,
    ) -> Result<(Option<CycleInfo>, PoSCycleStreamingStep), ModelsError> {
        let cycle_index = match cursor {
            PoSCycleStreamingStep::Started => self.get_first_cycle_index(),
            PoSCycleStreamingStep::Ongoing(last_cycle) => {
                if let Some(index) = self.get_cycle_index(last_cycle) {
                    if index == self.cycle_history.len() - 1 {
                        return Ok((None, PoSCycleStreamingStep::Finished));
                    }
                    index.saturating_add(1)
                } else {
                    return Err(ModelsError::OutdatedBootstrapCursor);
                }
            }
            PoSCycleStreamingStep::Finished => return Ok((None, PoSCycleStreamingStep::Finished)),
        };
        let cycle_info = self
            .cycle_history
            .get(cycle_index)
            .expect("a cycle should be available here");

        Ok((
            Some(cycle_info.clone()),
            PoSCycleStreamingStep::Ongoing(cycle_info.cycle),
        ))
    }

    /// Gets a part of the Proof of Stake `deferred_credits`. Used only in the bootstrap process.
    ///
    /// # Arguments:
    /// `cursor`: indicates the bootstrap state after the previous payload
    ///
    /// # Returns
    /// The PoS `deferred_credits` part and the updated cursor
    pub fn get_deferred_credits_part(
        &self,
        cursor: Option<Slot>,
    ) -> Result<(DeferredCredits, Option<Slot>), ModelsError> {
        let left_bound = if let Some(last_slot) = cursor {
            Excluded(last_slot)
        } else {
            Unbounded
        };
        let mut credits_part = DeferredCredits::default();
        for (slot, credits) in self.deferred_credits.0.range((left_bound, Unbounded)) {
            credits_part.0.insert(slot.clone(), credits.clone());
        }
        let last_credits_slot = credits_part.0.last_key_value().map(|(&slot, _)| slot);
        Ok((credits_part, last_credits_slot))
    }

    /// Sets a part of the Proof of Stake `cycle_history`. Used only in the bootstrap process.
    ///
    /// # Arguments
    /// `part`: the raw data received from `get_pos_state_part` and used to update PoS State
    pub fn set_cycle_history_part(
        &mut self,
        part: &[u8],
    ) -> Result<PoSCycleStreamingStep, ModelsError> {
        if part.is_empty() {
            return Ok(PoSCycleStreamingStep::Finished);
        }
        let u64_deser = U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX));
        let bitvec_deser = BitVecDeserializer::new();
        let address_deser = AddressDeserializer::new();
        #[allow(clippy::type_complexity)]
        let (rest, cycle): (
            &[u8], // non-deserialized buffer remainder
            (
                u64,                      // cycle
                bool,                     // complete
                Vec<(Address, u64)>,      // roll counts
                bitvec::vec::BitVec<u8>,  // seed
                Vec<(Address, u64, u64)>, // production stats (address, n_success, n_fail)
            ),
        ) = context(
            "cycle_history",
            tuple((
                context("cycle", |input| {
                    u64_deser.deserialize::<DeserializeError>(input)
                }),
                context(
                    "complete",
                    alt((value(true, tag(&[1])), value(false, tag(&[0])))),
                ),
                context(
                    "roll_counts",
                    length_count(
                        context("roll_counts length", |input| u64_deser.deserialize(input)),
                        tuple((
                            context("address", |input| address_deser.deserialize(input)),
                            context("count", |input| u64_deser.deserialize(input)),
                        )),
                    ),
                ),
                context("rng_seed", |input| bitvec_deser.deserialize(input)),
                context(
                    "production_stats",
                    length_count(
                        context("production_stats length", |input| {
                            u64_deser.deserialize(input)
                        }),
                        tuple((
                            context("address", |input| address_deser.deserialize(input)),
                            context("block_success_count", |input| u64_deser.deserialize(input)),
                            context("block_failure_count", |input| u64_deser.deserialize(input)),
                        )),
                    ),
                ),
            )),
        )
        .parse(part)
        .map_err(|err| ModelsError::DeserializeError(err.to_string()))?;

        if !rest.is_empty() {
            return Err(ModelsError::SerializeError(
                "data is left after set_cycle_history_part PoSFinalState part deserialization"
                    .to_string(),
            ));
        }

        let stats_iter =
            cycle
                .4
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

        if let Some(info) = self.cycle_history.back_mut() && info.cycle == cycle.0 {
            info.complete = cycle.1;
            info.roll_counts.extend(cycle.2);
            info.rng_seed.extend(cycle.3);
            info.production_stats.extend(stats_iter);
        } else {
            let opt_next_cycle = self.cycle_history.back().map(|info| info.cycle.saturating_add(1));
            if let Some(next_cycle) = opt_next_cycle && cycle.0 != next_cycle {
                if self.cycle_history.iter().map(|item| item.cycle).any(|x| x == cycle.0) {
                    warn!("PoS received cycle ({}) is already owned by the connecting node", cycle.0);
                }
                panic!("PoS received cycle ({}) should be equal to the next expected cycle ({})", cycle.0, next_cycle);
            }
            self.cycle_history.push_back(CycleInfo {
                cycle: cycle.0,
                complete: cycle.1,
                roll_counts: cycle.2.into_iter().collect(),
                rng_seed: cycle.3,
                production_stats: stats_iter.collect(),
            })
        }

        Ok(PoSCycleStreamingStep::Ongoing(
            self.cycle_history
                .back()
                .map(|v| v.cycle)
                .expect("should contain at least one cycle"),
        ))
    }

    /// Sets a part of the Proof of Stake `deferred_credits`. Used only in the bootstrap process.
    ///
    /// # Arguments
    /// `part`: the raw data received from `get_pos_state_part` and used to update PoS State
    pub fn set_deferred_credits_part(&mut self, part: &[u8]) -> Result<Option<Slot>, ModelsError> {
        if part.is_empty() {
            return Ok(self.deferred_credits.0.last_key_value().map(|(k, _)| *k));
        }
        #[allow(clippy::type_complexity)]
        let (rest, credits): (&[u8], Vec<(Slot, Vec<(Address, Amount)>)>) = context(
            "deferred_credits",
            length_count(
                context("deferred_credits length", |input| {
                    self.deferred_credit_length_deserializer.deserialize(input)
                }),
                tuple((
                    context("slot", |input| {
                        self.slot_deserializer
                            .deserialize::<DeserializeError>(input)
                    }),
                    context(
                        "credits",
                        length_count(
                            context("credits length", |input| {
                                self.deferred_credit_length_deserializer.deserialize(input)
                            }),
                            tuple((
                                context("address", |input| {
                                    self.address_deserializer.deserialize(input)
                                }),
                                context("amount", |input| {
                                    self.amount_deserializer.deserialize(input)
                                }),
                            )),
                        ),
                    ),
                )),
            ),
        )
        .parse(part)
        .map_err(|err| ModelsError::DeserializeError(err.to_string()))?;
        if !rest.is_empty() {
            return Err(ModelsError::SerializeError(
                "data is left after set_deferred_credits_part PoSFinalState part deserialization"
                    .to_string(),
            ));
        }

        let new_credits = DeferredCredits(
            credits
                .into_iter()
                .map(|(slot, credits)| (slot, credits.into_iter().collect()))
                .collect(),
        );
        self.deferred_credits.nested_extend(new_credits);

        Ok(self.deferred_credits.0.last_key_value().map(|(k, _)| *k))
    }
}
