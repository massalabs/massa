// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::ENDORSEMENT_ID_SIZE_BYTES;
use crate::node_configuration::THREAD_COUNT;
use crate::prehash::PreHashed;
use crate::wrapped::{Id, Wrapped, WrappedContent};
use crate::{BlockId, ModelsError, Slot};
use crate::{SlotDeserializer, SlotSerializer};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use nom::error::context;
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
use std::ops::Bound::{Excluded, Included};
use std::{fmt::Display, str::FromStr};

const ENDORSEMENT_ID_STRING_PREFIX: &str = "END";

/// endorsement id
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct EndorsementId(Hash);

impl PreHashed for EndorsementId {}

impl Id for EndorsementId {
    fn new(hash: Hash) -> Self {
        EndorsementId(hash)
    }

    fn hash(&self) -> Hash {
        self.0
    }
}

impl std::fmt::Display for EndorsementId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(
                f,
                "{}-{}",
                ENDORSEMENT_ID_STRING_PREFIX,
                self.0.to_bs58_check()
            )
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl FromStr for EndorsementId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(EndorsementId(Hash::from_str(s)?))
            } else if v[0] != ENDORSEMENT_ID_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    ENDORSEMENT_ID_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(EndorsementId(Hash::from_str(v[1])?))
            }
        } else {
            Ok(EndorsementId(Hash::from_str(s)?))
        }
    }
}

impl EndorsementId {
    /// endorsement id to bytes
    pub fn to_bytes(&self) -> &[u8; ENDORSEMENT_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// endorsement id into bytes
    pub fn into_bytes(self) -> [u8; ENDORSEMENT_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// endorsement id from bytes
    pub fn from_bytes(data: &[u8; ENDORSEMENT_ID_SIZE_BYTES]) -> EndorsementId {
        EndorsementId(Hash::from_bytes(data))
    }

    /// endorsement id from `bs58` check
    pub fn from_bs58_check(data: &str) -> Result<EndorsementId, ModelsError> {
        Ok(EndorsementId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }
}

impl Display for Endorsement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "Endorsed block: {} at slot {}",
            self.endorsed_block, self.slot
        )?;
        writeln!(f, "Index: {}", self.index)?;
        Ok(())
    }
}

/// an endorsement, as sent in the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsement {
    /// slot of endorsed block
    pub slot: Slot,
    /// endorsement index inside the block
    pub index: u32,
    /// hash of endorsed block
    pub endorsed_block: BlockId,
}

/// Wrapped endorsement
pub type WrappedEndorsement = Wrapped<Endorsement, EndorsementId>;

impl WrappedContent for Endorsement {}

/// Serializer for `Endorsement`
pub struct EndorsementSerializer {
    slot_serializer: SlotSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl EndorsementSerializer {
    /// Creates a new `EndorsementSerializer`
    pub fn new() -> Self {
        EndorsementSerializer {
            slot_serializer: SlotSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for EndorsementSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Endorsement> for EndorsementSerializer {
    fn serialize(&self, value: &Endorsement, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.slot_serializer.serialize(&value.slot, buffer)?;
        self.u32_serializer.serialize(&value.index, buffer)?;
        buffer.extend(value.endorsed_block.0.to_bytes());
        Ok(())
    }
}

/// Deserializer for `Endorsement`
pub struct EndorsementDeserializer {
    slot_deserializer: SlotDeserializer,
    u32_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl EndorsementDeserializer {
    /// Creates a new `EndorsementDeserializer`
    pub const fn new(endorsement_count: u32) -> Self {
        EndorsementDeserializer {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(THREAD_COUNT)),
            ),
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Excluded(endorsement_count)),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<Endorsement> for EndorsementDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Endorsement, E> {
        context(
            "Failed endorsement deserialization",
            tuple((
                context("Failed slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed index deserialization", |input| {
                    self.u32_deserializer.deserialize(input)
                }),
                context("Failed endorsed_block deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(slot, index, hash_block_id)| Endorsement {
            slot,
            index,
            endorsed_block: BlockId::new(hash_block_id),
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use crate::wrapped::{WrappedDeserializer, WrappedSerializer};

    use super::*;
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_endorsement_serialization() {
        let sender_keypair = KeyPair::generate();
        let content = Endorsement {
            slot: Slot::new(10, 1),
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement: WrappedEndorsement =
            Endorsement::new_wrapped(content, EndorsementSerializer::new(), &sender_keypair)
                .unwrap();

        let mut ser_endorsement: Vec<u8> = Vec::new();
        let serializer = WrappedSerializer::new();
        serializer
            .serialize(&endorsement, &mut ser_endorsement)
            .unwrap();
        let (_, res_endorsement): (&[u8], WrappedEndorsement) =
            WrappedDeserializer::new(EndorsementDeserializer::new(1))
                .deserialize::<DeserializeError>(&ser_endorsement)
                .unwrap();
        assert_eq!(
            format!("{:?}", res_endorsement),
            format!("{:?}", endorsement)
        );
    }
}
