//! Copyright (c) 2022 MASSA LABS <info@massa.net>

// use crate::endorsement::{EndorsementId, EndorsementSerializer, EndorsementSerializerLW};
// use crate::prehash::PreHashed;
use crate::secure_share::{
    Id, SecureShare, SecureShareContent, SecureShareDeserializer, SecureShareSerializer,
};
use crate::{
    // endorsement::{Endorsement, EndorsementDeserializerLW, SecureShareEndorsement},
    error::ModelsError,
    operation::{
        OperationId, OperationIdsDeserializer, OperationIdsSerializer, SecureShareOperation,
    },
    // slot::{Slot, SlotDeserializer, SlotSerializer},
};
// use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer};
use massa_signature::{KeyPair, PublicKey, Signature};
// use nom::branch::alt;
// use nom::bytes::complete::tag;
use nom::error::context;
// use nom::multi::{count, length_count};
use nom::sequence::{
    // preceded,
    tuple};
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
// use serde_with::{DeserializeFromStr, SerializeDisplay};
// use std::convert::TryInto;
use std::fmt::Formatter;
use std::ops::Bound::Included;
use nom::multi::length_count;
// use serde::de::value::U32Deserializer;
// use std::ops::Bound::{Excluded, Included};
// use std::str::FromStr;
use crate::denunciation::{Denunciation, DenunciationDeserializer, DenunciationSerializer};

// temp for compile
use crate::block_id::BlockId;
use crate::block_header::{BlockHeader, BlockHeaderDeserializer, SecuredHeader};

/// block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockV1 {
    /// signed header
    pub header: SecuredHeader,
    /// Dummy Denunciations
    pub denunciations: Vec<Denunciation>,
    /// operations ids
    pub operations: Vec<OperationId>,
}

/// filled block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilledBlockV1 {
    /// signed header
    pub header: SecuredHeader,
    /// Dummy Denunciations
    pub denunciations: Vec<Denunciation>,
    /// operations
    pub operations: Vec<(OperationId, Option<SecureShareOperation>)>,
}

/// Block with assosciated meta-data and interfaces allowing trust of data in untrusted network
pub type SecureShareBlock = SecureShare<BlockV1, BlockId>;

impl SecureShareContent for BlockV1 {
    fn new_verifiable<SC: Serializer<Self>, U: Id>(
        content: Self,
        content_serializer: SC,
        _keypair: &KeyPair,
    ) -> Result<SecureShare<Self, U>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        Ok(SecureShare {
            signature: content.header.signature,
            content_creator_pub_key: content.header.content_creator_pub_key,
            content_creator_address: content.header.content_creator_address,
            id: U::new(*content.header.id.get_hash()),
            content,
            serialized_data: content_serialized,
        })
    }

    fn serialize(
        _signature: &Signature,
        _creator_public_key: &PublicKey,
        serialized_content: &[u8],
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        buffer.extend(serialized_content);
        Ok(())
    }

    fn deserialize<
        'a,
        E: ParseError<&'a [u8]> + ContextError<&'a [u8]>,
        DC: Deserializer<Self>,
        U: Id,
    >(
        _content_serializer: Option<&dyn Serializer<Self>>,
        _signature_deserializer: &massa_signature::SignatureDeserializer,
        _creator_public_key_deserializer: &massa_signature::PublicKeyDeserializer,
        content_deserializer: &DC,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], SecureShare<Self, U>, E> {
        let (rest, content) = content_deserializer.deserialize(buffer)?;
        Ok((
            rest,
            SecureShare {
                signature: content.header.signature,
                content_creator_pub_key: content.header.content_creator_pub_key,
                content_creator_address: content.header.content_creator_address,
                id: U::new(*content.header.id.get_hash()),
                content,
                serialized_data: buffer[..buffer.len() - rest.len()].to_vec(),
            },
        ))
    }
}
/// Serializer for `Block`
pub struct BlockV1Serializer {
    header_serializer: SecureShareSerializer,
    len_serializer: U32VarIntSerializer,
    denunciation_serializer: DenunciationSerializer,
    op_ids_serializer: OperationIdsSerializer,
}

impl BlockV1Serializer {
    /// Creates a new `BlockSerializer`
    pub fn new() -> Self {
        BlockV1Serializer {
            header_serializer: SecureShareSerializer::new(),
            len_serializer: U32VarIntSerializer::new(),
            denunciation_serializer: DenunciationSerializer::new(),
            op_ids_serializer: OperationIdsSerializer::new(),
        }
    }
}

impl Default for BlockV1Serializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BlockV1> for BlockV1Serializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block::{Block, BlockSerializer, BlockId}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, secure_share::SecureShareContent, prehash::PreHashSet};
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///     .map(|i| BlockId(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_verifiable(
    ///     BlockHeader {
    ///         slot: Slot::new(1, 1),
    ///         parents,
    ///         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///         endorsements: vec![
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(4, 0),
    ///                     index: 3,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///         ],
    ///     },
    ///     BlockHeaderSerializer::new(),
    ///     &keypair,
    /// )
    /// .unwrap();
    ///
    /// // create block
    /// let orig_block = Block {
    ///     header: orig_header,
    ///     operations: Vec::new(),
    /// };
    ///
    /// let mut buffer = Vec::new();
    /// BlockSerializer::new().serialize(&orig_block, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &BlockV1, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.header_serializer.serialize(&value.header, buffer)?;
        // Denunciations
        let de_len: u32 = value.denunciations.len().try_into().map_err(|_| {
            SerializeError::NumberTooBig(
                "could not encode Vec<OperationId> denunciations length as u32".into(),
            )
        })?;
        self.len_serializer.serialize(&de_len, buffer)?;
        for de in value.denunciations.iter() {
            self.denunciation_serializer.serialize(de, buffer)?;
        }
        // Operations
        self.op_ids_serializer
            .serialize(&value.operations, buffer)?;
        Ok(())
    }
}

/// Deserializer for `Block`
pub struct BlockV1Deserializer {
    header_deserializer: SecureShareDeserializer<BlockHeader, BlockHeaderDeserializer>,
    len_deserializer: U32VarIntDeserializer,
    de_deserializer: DenunciationDeserializer,
    op_ids_deserializer: OperationIdsDeserializer,
}

impl BlockV1Deserializer {
    /// Creates a new `BlockDeserializer`
    pub fn new(thread_count: u8, max_operations_per_block: u32, endorsement_count: u32) -> Self {
        BlockV1Deserializer {
            header_deserializer: SecureShareDeserializer::new(BlockHeaderDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            len_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(9999), // FIXME
            ),
            de_deserializer: DenunciationDeserializer::new(thread_count, endorsement_count),
            op_ids_deserializer: OperationIdsDeserializer::new(max_operations_per_block),
        }
    }
}

impl Deserializer<BlockV1> for BlockV1Deserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block::{Block, BlockSerializer, BlockDeserializer, BlockId}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, secure_share::SecureShareContent, prehash::PreHashSet};
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///     .map(|i| BlockId(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_verifiable(
    ///     BlockHeader {
    ///         slot: Slot::new(1, 1),
    ///         parents,
    ///         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///         endorsements: vec![
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(4, 0),
    ///                     index: 3,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///         ],
    ///     },
    ///     BlockHeaderSerializer::new(),
    ///     &keypair,
    /// )
    /// .unwrap();
    ///
    /// // create block
    /// let orig_block = Block {
    ///     header: orig_header,
    ///     operations: Vec::new(),
    /// };
    ///
    /// let mut buffer = Vec::new();
    /// BlockSerializer::new().serialize(&orig_block, &mut buffer).unwrap();
    /// let (rest, res_block) = BlockDeserializer::new(THREAD_COUNT, 100, 9).deserialize::<DeserializeError>(&mut buffer).unwrap();
    ///
    /// assert!(rest.is_empty());
    /// // check equality
    /// assert_eq!(orig_block.header.id, res_block.header.id);
    /// assert_eq!(
    ///     orig_block.header.content.slot,
    ///     res_block.header.content.slot
    /// );
    /// assert_eq!(
    ///     orig_block.header.serialized_data,
    ///     res_block.header.serialized_data
    /// );
    /// assert_eq!(
    ///     orig_block.header.signature,
    ///     res_block.header.signature
    /// );
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockV1, E> {
        context(
            "Failed Block deserialization",
            tuple((
                context("Failed header deserialization", |input| {
                    self.header_deserializer.deserialize(input)
                }),
                length_count(
                    context("Failed length deserialization", |input| {
                        self.len_deserializer.deserialize(input)
                    }),
                    context("Failed Denunciation deserialization", |input| {
                        self.de_deserializer.deserialize(input)
                    }),
                ),
                context("Failed operations deserialization", |input| {
                    self.op_ids_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(header, denunciations, operations)| BlockV1 { header, denunciations, operations })
        .parse(buffer)
    }
}

impl SecureShareBlock {
    /// size in bytes of the whole block
    pub fn bytes_count(&self) -> u64 {
        self.serialized_data.len() as u64
    }

    /// true if given operation is included in the block
    pub fn contains_operation(&self, op: SecureShareOperation) -> bool {
        self.content.operations.contains(&op.id)
    }

    /// returns the fitness of the block
    pub fn get_fitness(&self) -> u64 {
        self.content.header.get_fitness()
    }
}

impl std::fmt::Display for BlockV1 {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.header)?;
        writeln!(
            f,
            "Operations: {}",
            self.operations
                .iter()
                .map(|op| format!("{}", op))
                .collect::<Vec<String>>()
                .join(" ")
        )?;
        Ok(())
    }
}