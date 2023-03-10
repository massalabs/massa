// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::prehash::PreHashed;
use crate::secure_share::{Id, SecureShare, SecureShareContent};
use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
use crate::{block_id::BlockId, error::ModelsError};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U32VarIntDeserializer,
    U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::context;
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::ops::Bound::{Excluded, Included};
use std::{fmt::Display, str::FromStr};

/// Endorsement ID size in bytes
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// endorsement id
#[derive(
    Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, SerializeDisplay, DeserializeFromStr,
)]
pub struct EndorsementId(Hash);

const ENDORSEMENTID_PREFIX: char = 'E';
const ENDORSEMENTID_VERSION: u64 = 0;

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
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&ENDORSEMENTID_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            ENDORSEMENTID_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for EndorsementId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for EndorsementId {
    type Err = ModelsError;
    /// ## Example
    /// ```rust
    /// # use massa_hash::Hash;
    /// # use std::str::FromStr;
    /// # use massa_models::endorsement::EndorsementId;
    /// # let endo_id = EndorsementId::from_bytes(&[0; 32]);
    /// let ser = endo_id.to_string();
    /// let res_endo_id = EndorsementId::from_str(&ser).unwrap();
    /// assert_eq!(endo_id, res_endo_id);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == ENDORSEMENTID_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::EndorsementIdParseError)?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::EndorsementIdParseError)?;
                Ok(EndorsementId(Hash::from_bytes(
                    rest.try_into()
                        .map_err(|_| ModelsError::EndorsementIdParseError)?,
                )))
            }
            _ => Err(ModelsError::EndorsementIdParseError),
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
    /// Slot in which the endorsement can be included
    pub slot: Slot,
    /// Endorsement index inside the including block
    pub index: u32,
    /// Hash of endorsed block.
    /// This is the parent in thread `self.slot.thread` of the block in which the endorsement is included
    pub endorsed_block: BlockId,
}

#[cfg(any(test, feature = "testing"))]
impl SecureShareEndorsement {
    // TODO: gh-issue #3398
    /// Used under testing conditions to validate an instance of Self
    pub fn check_invariants(&self) -> Result<(), Box<dyn std::error::Error>> {
        if let Err(e) = self.verify_signature() {
            return Err(e.into());
        }
        if self.content.slot.thread >= crate::config::THREAD_COUNT {
            Err("Endorsement slot on non-existant thread".into())
        } else if self.content.index >= crate::config::ENDORSEMENT_COUNT {
            Err("Endorsement index out of range".into())
        } else {
            Ok(())
        }
    }
}

/// Wrapped endorsement
pub type SecureShareEndorsement = SecureShare<Endorsement, EndorsementId>;

impl SecureShareContent for Endorsement {
    /// Compute the ID of the non-malleable contents.
    /// This specialization allows for compact multi-stake denunciation.
    ///
    /// # Arguments
    /// * content: reference to the content (useful for example for denunciable objects)
    /// * non_malleable_content_serialized: a reference to the non-malleable serialized content to be used for ID computation. If malleability on some fields is needed, they should not be included.
    /// * content_creator_pub_key: reference to the public key of the content creator
    fn compute_id<ID: Id>(
        content: &Self,
        non_malleable_content_serialized: &[u8],
        content_creator_pub_key: &PublicKey,
    ) -> ID {
        // ID format: H(content_creator_pub_key | slot | index | H(nonmalleable_contents_serialized))
        let mut hash_data = Vec::new();
        hash_data.extend(content_creator_pub_key.to_bytes());
        hash_data.extend(content.slot.to_bytes_key()); // fixed-length prefixes are safer when we don't have separators in the hashed data concatenation
        hash_data.extend(content.index.to_be_bytes()); // fixed-length prefixes are safer when we don't have separators in the hashed data concatenation
        hash_data.extend(Hash::compute_from(non_malleable_content_serialized).to_bytes());
        ID::new(Hash::compute_from(&hash_data))
    }
}

/// Serializer for `Endorsement`
#[derive(Clone)]
pub struct EndorsementSerializer {
    slot_serializer: SlotSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl EndorsementSerializer {
    /// Creates a new `EndorsementSerializer`
    pub const fn new() -> Self {
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
    /// use massa_models::{slot::Slot, block_id::BlockId, endorsement::{Endorsement, EndorsementSerializer}};
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
    /// use massa_models::{slot::Slot, block_id::BlockId, endorsement::{Endorsement, EndorsementSerializer, EndorsementDeserializer}};
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

/// Lightweight Serializer for `Endorsement`
/// When included in a `BlockHeader`, we want to serialize only the index (optimization)
pub struct EndorsementSerializerLW {
    // slot_serializer: SlotSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl EndorsementSerializerLW {
    /// Creates a new `EndorsementSerializerLW`
    pub fn new() -> Self {
        EndorsementSerializerLW {
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
    /// use massa_models::{slot::Slot, block_id::BlockId, endorsement::{Endorsement, EndorsementSerializerLW}};
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
        self.u32_serializer.serialize(&value.index, buffer)?;
        Ok(())
    }
}

/// Lightweight Deserializer for `Endorsement`
pub struct EndorsementDeserializerLW {
    index_deserializer: U32VarIntDeserializer,
    slot: Slot,
    endorsed_block: BlockId,
}

impl EndorsementDeserializerLW {
    /// Creates a new `EndorsementDeserializerLW`
    pub const fn new(endorsement_count: u32, slot: Slot, endorsed_block: BlockId) -> Self {
        EndorsementDeserializerLW {
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(endorsement_count),
            ),
            slot,
            endorsed_block,
        }
    }
}

impl Deserializer<Endorsement> for EndorsementDeserializerLW {
    /// ## Example:
    /// ```rust
    /// use massa_models::{slot::Slot, block_id::BlockId, endorsement::{Endorsement, EndorsementSerializerLW, EndorsementDeserializerLW}};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_hash::Hash;
    ///
    /// let slot = Slot::new(1, 2);
    /// let endorsed_block = BlockId(Hash::compute_from("test".as_bytes()));
    /// let endorsement = Endorsement {
    ///   slot: slot,
    ///   index: 0,
    ///   endorsed_block: endorsed_block
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializerLW::new().serialize(&endorsement, &mut buffer).unwrap();
    /// let (rest, deserialized) = EndorsementDeserializerLW::new(10, slot, endorsed_block).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// assert_eq!(deserialized.index, endorsement.index);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Endorsement, E> {
        context("Failed index deserialization", |input| {
            self.index_deserializer.deserialize(input)
        })
        .map(|index| Endorsement {
            slot: self.slot,
            index,
            endorsed_block: self.endorsed_block,
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use crate::secure_share::{SecureShareDeserializer, SecureShareSerializer};

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
        let endorsement: SecureShareEndorsement =
            Endorsement::new_verifiable(content, EndorsementSerializer::new(), &sender_keypair)
                .unwrap();

        let mut ser_endorsement: Vec<u8> = Vec::new();
        let serializer = SecureShareSerializer::new();
        serializer
            .serialize(&endorsement, &mut ser_endorsement)
            .unwrap();
        let (_, res_endorsement): (&[u8], SecureShareEndorsement) =
            SecureShareDeserializer::new(EndorsementDeserializer::new(32, 1))
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
        let endorsement: SecureShareEndorsement =
            Endorsement::new_verifiable(content, EndorsementSerializerLW::new(), &sender_keypair)
                .unwrap();

        let mut ser_endorsement: Vec<u8> = Vec::new();
        let serializer = SecureShareSerializer::new();
        serializer
            .serialize(&endorsement, &mut ser_endorsement)
            .unwrap();

        let parent = BlockId(Hash::compute_from("blk".as_bytes()));

        let (_, res_endorsement): (&[u8], SecureShareEndorsement) = SecureShareDeserializer::new(
            EndorsementDeserializerLW::new(1, Slot::new(10, 1), parent),
        )
        .deserialize::<DeserializeError>(&ser_endorsement)
        .unwrap();
        // Test only endorsement index as with the lw ser. we only process this field
        assert_eq!(res_endorsement.content.index, endorsement.content.index);
    }
}
