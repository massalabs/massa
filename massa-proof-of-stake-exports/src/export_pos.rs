use std::collections::VecDeque;
use std::ops::Bound::Included;

use massa_models::constants::{MAX_BOOTSTRAP_POS_CYCLES, THREAD_COUNT};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    multi::{count, length_count},
    IResult, Parser,
};
use serde::{Deserialize, Serialize};

use crate::{
    proof_of_stake::ProofOfStake,
    thread_cycle_state::{
        ThreadCycleState, ThreadCycleStateDeserializer, ThreadCycleStateSerializer,
    },
};
/// serializable version of the proof of stake
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportProofOfStake {
    /// Index by thread and cycle number
    pub cycle_states: Vec<VecDeque<ThreadCycleState>>,
}

impl From<&ProofOfStake> for ExportProofOfStake {
    fn from(pos: &ProofOfStake) -> Self {
        ExportProofOfStake {
            cycle_states: pos.cycle_states.clone(),
        }
    }
}

/// Serializer for `ExportProofOfStake`
pub struct ExportProofOfStakeSerializer {
    u32_serializer: U32VarIntSerializer,
    cycle_state_serializer: ThreadCycleStateSerializer,
}

impl ExportProofOfStakeSerializer {
    /// Creates a `ExportProofOfStakeSerializer`
    pub fn new() -> Self {
        Self {
            u32_serializer: U32VarIntSerializer::new(),
            cycle_state_serializer: ThreadCycleStateSerializer::new(),
        }
    }
}

impl Default for ExportProofOfStakeSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<ExportProofOfStake> for ExportProofOfStakeSerializer {
    /// ## Example
    /// ```rust
    /// use bitvec::prelude::BitVec;
    /// use massa_proof_of_stake_exports::{ExportProofOfStake, ExportProofOfStakeSerializer, ThreadCycleState};
    /// use massa_models::{Address, Slot, prehash::Map, rolls::{RollUpdate, RollUpdates, RollCounts}, constants::THREAD_COUNT};
    /// use std::str::FromStr;
    /// use std::collections::VecDeque;
    /// use massa_serialization::Serializer;
    ///
    /// let mut thread_cycle_state = ThreadCycleState {
    ///   cycle: 0,
    ///   last_final_slot: Slot::new(1, 2),
    ///   roll_count: RollCounts::default(),
    ///   cycle_updates: RollUpdates::default(),
    ///   rng_seed: BitVec::default(),
    ///   production_stats: Map::default(),
    /// };
    /// thread_cycle_state.roll_count.0.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   1,
    /// );
    /// thread_cycle_state.roll_count.0.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   4,
    /// );
    /// thread_cycle_state.cycle_updates.0.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   RollUpdate {
    ///    roll_purchases: 3,
    ///    roll_sales: 0
    ///   }
    /// );
    /// thread_cycle_state.production_stats.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   (1, 2)
    /// );
    /// let mut thread_cycle_state_vec = VecDeque::new();
    /// thread_cycle_state_vec.push_back(thread_cycle_state.clone());
    /// thread_cycle_state_vec.push_back(thread_cycle_state.clone());
    /// let mut export_proof_of_stake = ExportProofOfStake {
    ///  cycle_states: vec![],
    /// };
    /// for _ in 0..THREAD_COUNT {
    ///   export_proof_of_stake.cycle_states.push(thread_cycle_state_vec.clone());
    /// }
    /// let mut buffer = Vec::new();
    /// ExportProofOfStakeSerializer::new().serialize(&export_proof_of_stake, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &ExportProofOfStake,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        for thread_lst in value.cycle_states.iter() {
            let cycle_count: u32 = thread_lst.len().try_into().map_err(|err| {
                SerializeError::NumberTooBig(format!(
                    "too many cycles when serializing ExportProofOfStake: {}",
                    err
                ))
            })?;
            self.u32_serializer.serialize(&cycle_count, buffer)?;
            for cycle_state in thread_lst.iter() {
                self.cycle_state_serializer.serialize(cycle_state, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `ExportProofOfStake`
pub struct ExportProofOfStakeDeserializer {
    u32_deserializer: U32VarIntDeserializer,
    thread_count: u8,
    cycle_state_deserializer: ThreadCycleStateDeserializer,
}

impl ExportProofOfStakeDeserializer {
    /// Creates a `ExportProofOfStakeDeserializer`
    pub fn new() -> Self {
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_POS_CYCLES),
            ),
            cycle_state_deserializer: ThreadCycleStateDeserializer::new(),
            thread_count,
        }
    }
}

impl Default for ExportProofOfStakeDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<ExportProofOfStake> for ExportProofOfStakeDeserializer {
    /// ## Example
    /// ```rust
    /// use bitvec::prelude::BitVec;
    /// use massa_proof_of_stake_exports::{ExportProofOfStake, ExportProofOfStakeSerializer, ExportProofOfStakeDeserializer, ThreadCycleState};
    /// use massa_models::{Address, Slot, prehash::Map, rolls::{RollUpdate, RollUpdates, RollCounts}, constants::THREAD_COUNT};
    /// use std::str::FromStr;
    /// use std::collections::VecDeque;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let mut thread_cycle_state = ThreadCycleState {
    ///   cycle: 0,
    ///   last_final_slot: Slot::new(1, 2),
    ///   roll_count: RollCounts::default(),
    ///   cycle_updates: RollUpdates::default(),
    ///   rng_seed: BitVec::default(),
    ///   production_stats: Map::default(),
    /// };
    /// thread_cycle_state.roll_count.0.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   1,
    /// );
    /// thread_cycle_state.roll_count.0.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   4,
    /// );
    /// thread_cycle_state.cycle_updates.0.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   RollUpdate {
    ///    roll_purchases: 3,
    ///    roll_sales: 0
    ///   }
    /// );
    /// thread_cycle_state.production_stats.insert(
    ///   Address::from_str("A12Xng6ydrtsX2smz8VVG5bvs6TmbNQYgv765UtsC2bm7htTgYW4").unwrap(),
    ///   (1, 2)
    /// );
    /// let mut thread_cycle_state_vec = VecDeque::new();
    /// thread_cycle_state_vec.push_back(thread_cycle_state.clone());
    /// thread_cycle_state_vec.push_back(thread_cycle_state.clone());
    /// let mut export_proof_of_stake = ExportProofOfStake {
    ///  cycle_states: vec![],
    /// };
    /// for _ in 0..THREAD_COUNT {
    ///   export_proof_of_stake.cycle_states.push(thread_cycle_state_vec.clone());
    /// }
    /// let mut buffer = Vec::new();
    /// ExportProofOfStakeSerializer::new().serialize(&export_proof_of_stake, &mut buffer).unwrap();
    /// let (rest, export_proof_of_stake_deserialized) = ExportProofOfStakeDeserializer::new().deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// let mut buffer2 = Vec::new();
    /// ExportProofOfStakeSerializer::new().serialize(&export_proof_of_stake_deserialized, &mut buffer2).unwrap();
    /// assert_eq!(buffer, buffer2);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExportProofOfStake, E> {
        context(
            "Failed ExportProofOfStake deserialization",
            count(
                length_count(
                    context("Failed length deserializer", |input| {
                        self.u32_deserializer.deserialize(input)
                    }),
                    context("Failed cycle_state deserializer", |input| {
                        self.cycle_state_deserializer.deserialize(input)
                    }),
                )
                .map(|cycles| cycles.into_iter().collect()),
                self.thread_count.into(),
            ),
        )
        .map(|cycle_states| ExportProofOfStake { cycle_states })
        .parse(buffer)
    }
}
