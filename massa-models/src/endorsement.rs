// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::block_id::{BlockIdDeserializer, BlockIdSerializer};
use crate::prehash::PreHashed;
use crate::secure_share::{Id, SecureShare, SecureShareContent};
use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
use crate::{block_id::BlockId, error::ModelsError};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U32VarIntDeserializer,
    U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::PublicKey;
use nom::error::{context, ErrorKind};
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::fmt::Formatter;
use std::ops::Bound::{Excluded, Included};
use std::{fmt::Display, str::FromStr};
use transition::Versioned;

/// Endorsement ID size in bytes
pub const ENDORSEMENT_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// endorsement id
#[allow(missing_docs)]
#[transition::versioned(versions("0"))]
#[derive(
    Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, SerializeDisplay, DeserializeFromStr,
)]
pub struct EndorsementId(Hash);

const ENDORSEMENTID_PREFIX: char = 'E';
const ENDORSEMENTID_VERSION: u64 = 0;

impl PreHashed for EndorsementId {}

impl Id for EndorsementId {
    fn new(hash: Hash) -> Self {
        EndorsementId::EndorsementIdV0(EndorsementIdV0(hash))
    }

    fn get_hash(&self) -> &Hash {
        match self {
            EndorsementId::EndorsementIdV0(endorsement_id) => endorsement_id.get_hash(),
        }
    }
}

#[transition::impl_version(versions("0"))]
impl EndorsementId {
    fn get_hash(&self) -> &Hash {
        &self.0
    }
}

impl std::fmt::Display for EndorsementId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EndorsementId::EndorsementIdV0(id) => write!(f, "{}", id),
        }
    }
}

#[transition::impl_version(versions("0"))]
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
    /// # use crate::massa_models::secure_share::Id;
    /// # let endo_id = EndorsementId::new(Hash::compute_from("endo_id".as_bytes()));
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
                let endorsement_id_deserializer = EndorsementIdDeserializer::new();
                let (rest, endorsement_id) = endorsement_id_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::EndorsementIdParseError)?;
                if rest.is_empty() {
                    Ok(endorsement_id)
                } else {
                    Err(ModelsError::EndorsementIdParseError)
                }
            }
            _ => Err(ModelsError::EndorsementIdParseError),
        }
    }
}

#[transition::impl_version(versions("0"))]
impl FromStr for EndorsementId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == ENDORSEMENTID_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::EndorsementIdParseError)?;
                let endorsement_id_deserializer = EndorsementIdDeserializer::new();
                let (rest, endorsement_id) = endorsement_id_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::EndorsementIdParseError)?;
                if rest.is_empty() {
                    Ok(endorsement_id)
                } else {
                    Err(ModelsError::EndorsementIdParseError)
                }
            }
            _ => Err(ModelsError::EndorsementIdParseError),
        }
    }
}

struct EndorsementIdDeserializer {
    version_deserializer: U64VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl EndorsementIdDeserializer {
    pub fn new() -> Self {
        Self {
            version_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<EndorsementId> for EndorsementIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], EndorsementId, E> {
        // Verify that we at least have a version and something else
        if buffer.len() < 2 {
            return Err(nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof)));
        }
        let (rest, endorsement_id_version) = self
            .version_deserializer
            .deserialize(buffer)
            .map_err(|_: nom::Err<E>| {
                nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof))
            })?;
        match endorsement_id_version {
            <EndorsementId!["0"]>::VERSION => {
                let (rest, endorsement_id) = self.deserialize(rest)?;
                Ok((rest, EndorsementIdVariant!["0"](endorsement_id)))
            }
            _ => Err(nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof))),
        }
    }
}

#[transition::impl_version(versions("0"), structures("EndorsementId"))]
impl Deserializer<EndorsementId> for EndorsementIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], EndorsementId, E> {
        context("Failed OperationId deserialization", |input| {
            self.hash_deserializer.deserialize(input)
        })
        .map(EndorsementId)
        .parse(buffer)
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
    /// Compute the signed hash
    fn compute_signed_hash(&self, public_key: &PublicKey, content_hash: &Hash) -> Hash {
        let mut signed_data: Vec<u8> = Vec::new();
        signed_data.extend(public_key.to_bytes());
        signed_data.extend(EndorsementDenunciationData::new(self.slot, self.index).to_bytes());
        signed_data.extend(content_hash.to_bytes());
        Hash::compute_from(&signed_data)
    }
}

/// Serializer for `Endorsement`
#[derive(Clone)]
pub struct EndorsementSerializer {
    slot_serializer: SlotSerializer,
    u32_serializer: U32VarIntSerializer,
    block_id_serializer: BlockIdSerializer,
}

impl EndorsementSerializer {
    /// Creates a new `EndorsementSerializer`
    pub fn new() -> Self {
        EndorsementSerializer {
            slot_serializer: SlotSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
            block_id_serializer: BlockIdSerializer::new(),
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
    ///   endorsed_block: BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()))
    /// };
    /// let mut buffer = Vec::new();
    /// EndorsementSerializer::new().serialize(&endorsement, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &Endorsement, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.slot_serializer.serialize(&value.slot, buffer)?;
        self.u32_serializer.serialize(&value.index, buffer)?;
        self.block_id_serializer
            .serialize(&value.endorsed_block, buffer)?;
        Ok(())
    }
}

/// Deserializer for `Endorsement`
pub struct EndorsementDeserializer {
    slot_deserializer: SlotDeserializer,
    index_deserializer: U32VarIntDeserializer,
    block_id_deserializer: BlockIdDeserializer,
}

impl EndorsementDeserializer {
    /// Creates a new `EndorsementDeserializer`
    pub fn new(thread_count: u8, endorsement_count: u32) -> Self {
        EndorsementDeserializer {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            index_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(endorsement_count),
            ),
            block_id_deserializer: BlockIdDeserializer::new(),
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
    ///   endorsed_block: BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()))
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
                    self.block_id_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(slot, index, endorsed_block)| Endorsement {
            slot,
            index,
            endorsed_block,
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
    ///   endorsed_block: BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()))
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
    /// let endorsed_block = BlockId::generate_from_hash(Hash::compute_from("test".as_bytes()));
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

/// A denunciation data for endorsement
#[derive(Debug)]
pub struct EndorsementDenunciationData {
    slot: Slot,
    index: u32,
}

impl EndorsementDenunciationData {
    /// Create a new denunciation data for endorsement
    pub fn new(slot: Slot, index: u32) -> Self {
        Self { slot, index }
    }

    /// Get byte array
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend(self.slot.to_bytes_key());
        buf.extend(self.index.to_le_bytes());
        buf
    }
}

#[cfg(test)]
mod tests {
    use crate::secure_share::{SecureShareContent, SecureShareDeserializer, SecureShareSerializer};
    use massa_signature::verify_signature_batch;

    use super::*;
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_endorsement_serialization() {
        let sender_keypair = KeyPair::generate(0).unwrap();
        let content = Endorsement {
            slot: Slot::new(10, 1),
            index: 0,
            endorsed_block: BlockId::generate_from_hash(Hash::compute_from("blk".as_bytes())),
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
        let sender_keypair = KeyPair::generate(0).unwrap();
        let content = Endorsement {
            slot: Slot::new(10, 1),
            index: 0,
            endorsed_block: BlockId::generate_from_hash(Hash::compute_from("blk".as_bytes())),
        };
        let endorsement: SecureShareEndorsement =
            Endorsement::new_verifiable(content, EndorsementSerializerLW::new(), &sender_keypair)
                .unwrap();

        let mut ser_endorsement: Vec<u8> = Vec::new();
        let serializer = SecureShareSerializer::new();
        serializer
            .serialize(&endorsement, &mut ser_endorsement)
            .unwrap();

        let parent = BlockId::generate_from_hash(Hash::compute_from("blk".as_bytes()));

        let (_, res_endorsement): (&[u8], SecureShareEndorsement) = SecureShareDeserializer::new(
            EndorsementDeserializerLW::new(1, Slot::new(10, 1), parent),
        )
        .deserialize::<DeserializeError>(&ser_endorsement)
        .unwrap();
        // Test only endorsement index as with the lw ser. we only process this field
        assert_eq!(res_endorsement.content.index, endorsement.content.index);
    }

    #[test]
    fn test_verify_sig_batch() {
        // test verify_signature_batch as we override SecureShareEndorsements compute_hash

        let sender_keypair = KeyPair::generate(0).unwrap();
        let content_1 = Endorsement {
            slot: Slot::new(10, 1),
            index: 0,
            endorsed_block: BlockId::generate_from_hash(Hash::compute_from("blk1".as_bytes())),
        };
        let s_endorsement_1: SecureShareEndorsement =
            Endorsement::new_verifiable(content_1, EndorsementSerializer::new(), &sender_keypair)
                .unwrap();
        let mut serialized = vec![];
        SecureShareSerializer::new()
            .serialize(&s_endorsement_1, &mut serialized)
            .unwrap();
        let (_, s_endorsement_1): (&[u8], SecureShare<Endorsement, EndorsementId>) =
            SecureShareDeserializer::new(EndorsementDeserializer::new(32, 32))
                .deserialize::<DeserializeError>(&serialized)
                .unwrap();
        let sender_keypair = KeyPair::generate(0).unwrap();
        let content_2 = Endorsement {
            slot: Slot::new(2, 5),
            index: 0,
            endorsed_block: BlockId::generate_from_hash(Hash::compute_from("blk2".as_bytes())),
        };
        let s_endorsement_2: SecureShareEndorsement =
            Endorsement::new_verifiable(content_2, EndorsementSerializerLW::new(), &sender_keypair)
                .unwrap();

        // Test with batch len == 1 (no // verif)
        let batch_1 = [(
            s_endorsement_1.compute_signed_hash(),
            s_endorsement_1.signature,
            s_endorsement_1.content_creator_pub_key,
        )];
        verify_signature_batch(&batch_1).unwrap();

        // Test with batch len > 1 (// verif)
        let batch_2 = [
            (
                s_endorsement_1.compute_signed_hash(),
                s_endorsement_1.signature,
                s_endorsement_1.content_creator_pub_key,
            ),
            (
                s_endorsement_2.compute_signed_hash(),
                s_endorsement_2.signature,
                s_endorsement_2.content_creator_pub_key,
            ),
        ];
        verify_signature_batch(&batch_2).unwrap();
    }
}
