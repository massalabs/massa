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
        Self {
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_BOOTSTRAP_POS_CYCLES),
            ),
            cycle_state_deserializer: ThreadCycleStateDeserializer::new(),
            thread_count: THREAD_COUNT,
        }
    }
}

impl Default for ExportProofOfStakeDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<ExportProofOfStake> for ExportProofOfStakeDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], ExportProofOfStake, E> {
        context(
            "Failed ExportProofOfStake deserialization",
            count(
                length_count(
                    |input| self.u32_deserializer.deserialize(input),
                    |input| self.cycle_state_deserializer.deserialize(input),
                )
                .map(|cycles| cycles.into_iter().collect()),
                self.thread_count.into(),
            ),
        )
        .map(|cycle_states| ExportProofOfStake { cycle_states })
        .parse(buffer)
    }
}
