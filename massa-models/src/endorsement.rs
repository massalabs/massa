// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::prehash::PreHashed;
use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
use crate::wrapped::{Id, Wrapped, WrappedContent};
use crate::{block::BlockId, error::ModelsError};
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

/// Endorsement ID size in bytes
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// endorsement id
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct EndorsementId(Hash);

impl PreHashed for EndorsementId {}

impl Id for EndorsementId {
    fn new(hash: Hash) -> Self {
        EndorsementId(hash)
    }

    fn get_hash(&self) -> &Hash {
        &self.0
    }
}

impl std::fmt::Display for EndorsementId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.0.to_bs58_check())
    }
}

impl FromStr for EndorsementId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(EndorsementId(Hash::from_str(s)?))
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block::BlockId, endorsement::{Endorsement, EndorsementSerializer}};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    ///
    /// let endorsement = Endorsement {
    ///   slot: Slot::new(1, 2),
    ///   index: 0,
    ///   endorsed_block: BlockId(Hash::compute_from("test".as_bytes()))
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializer::new().serialize(&endorsement, &mut buffer).unwrap();
    /// ```
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
    index_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl EndorsementDeserializer {
    /// Creates a new `EndorsementDeserializer`
    pub const fn new(thread_count: u8, endorsement_count: u32) -> Self {
        EndorsementDeserializer {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(endorsement_count),
            ),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<Endorsement> for EndorsementDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block::BlockId, endorsement::{Endorsement, EndorsementSerializer, EndorsementDeserializer}};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_hash::Hash;
    ///
    /// let endorsement = Endorsement {
    ///   slot: Slot::new(1, 2),
    ///   index: 0,
    ///   endorsed_block: BlockId(Hash::compute_from("test".as_bytes()))
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializer::new().serialize(&endorsement, &mut buffer).unwrap();
    /// let (rest, deserialized) = EndorsementDeserializer::new(32, 10).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized.slot, endorsement.slot);
    /// assert_eq!(deserialized.index, endorsement.index);
    /// assert_eq!(deserialized.endorsed_block, endorsement.endorsed_block);
    /// ```
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
                    self.index_deserializer.deserialize(input)
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

/// LightWeight Serializer for `Endorsement`
/// When included in a BlockHeader, we want to serialize only the index (optim)
pub struct EndorsementSerializerLW {
    // slot_serializer: SlotSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl EndorsementSerializerLW {
    /// Creates a new `EndorsementSerializerLW`
    pub fn new() -> Self {
        EndorsementSerializerLW {
            // slot_serializer: SlotSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
        }
    }
}

impl Default for EndorsementSerializerLW {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Endorsement> for EndorsementSerializerLW {
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block::BlockId, endorsement::{Endorsement, EndorsementSerializerLW}};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    ///
    /// let endorsement = Endorsement {
    ///   slot: Slot::new(1, 2),
    ///   index: 0,
    ///   endorsed_block: BlockId(Hash::compute_from("test".as_bytes()))
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializerLW::new().serialize(&endorsement, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &Endorsement, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // self.slot_serializer.serialize(&value.slot, buffer)?;
        self.u32_serializer.serialize(&value.index, buffer)?;
        // buffer.extend(value.endorsed_block.0.to_bytes());
        Ok(())
    }
}

/// Lightweight Deserializer for `Endorsement`
pub struct EndorsementDeserializerLW {
    index_deserializer: U32VarIntDeserializer,
}

impl EndorsementDeserializerLW {
    /// Creates a new `EndorsementDeserializerLW`
    pub const fn new(endorsement_count: u32) -> Self {
        EndorsementDeserializerLW {
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(endorsement_count),
            ),
        }
    }
}

impl Deserializer<Endorsement> for EndorsementDeserializerLW {
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block::BlockId, endorsement::{Endorsement, EndorsementSerializerLW, EndorsementDeserializerLW}};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_hash::Hash;
    ///
    /// let endorsement = Endorsement {
    ///   slot: Slot::new(1, 2),
    ///   index: 0,
    ///   endorsed_block: BlockId(Hash::compute_from("test".as_bytes()))
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializerLW::new().serialize(&endorsement, &mut buffer).unwrap();
    /// let (rest, deserialized) = EndorsementDeserializerLW::new(10).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized.index, endorsement.index);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Endorsement, E> {
        println!("[EndorsementDeserializerLW] start");
        let hash_default = Hash::from_bytes(&[0; 32]);
        context(
            "Failed endorsement deserialization",
            tuple((context("Failed index deserialization", |input| {
                self.index_deserializer.deserialize(input)
            }),)),
        )
        .map(|(index,)| Endorsement {
            slot: Slot::new(0, 0),
            index,
            endorsed_block: BlockId::new(hash_default),
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
            WrappedDeserializer::new(EndorsementDeserializer::new(32, 1))
                .deserialize::<DeserializeError>(&ser_endorsement)
                .unwrap();
        assert_eq!(res_endorsement, endorsement);
    }

    #[test]
    #[serial]
    fn test_endorsement_lightweight_serialization() {
        let sender_keypair = KeyPair::generate();
        let content = Endorsement {
            slot: Slot::new(10, 1),
            index: 0,
            endorsed_block: BlockId(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement: WrappedEndorsement =
            Endorsement::new_wrapped(content, EndorsementSerializerLW::new(), &sender_keypair)
                .unwrap();

        let mut ser_endorsement: Vec<u8> = Vec::new();
        let serializer = WrappedSerializer::new();
        serializer
            .serialize(&endorsement, &mut ser_endorsement)
            .unwrap();

        let (_, res_endorsement): (&[u8], WrappedEndorsement) =
            WrappedDeserializer::new(EndorsementDeserializerLW::new(1))
                .deserialize::<DeserializeError>(&ser_endorsement)
                .unwrap();
        // Test only endorsement index as with the lw ser. we only process this field
        assert_eq!(res_endorsement.content.index, endorsement.content.index);
    }
}
