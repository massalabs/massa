//! Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::endorsement::{EndorsementId, EndorsementSerializer, EndorsementSerializerLW};
use crate::prehash::PreHashed;
use crate::wrapped::{Id, Wrapped, WrappedContent, WrappedDeserializer, WrappedSerializer};
use crate::{
    endorsement::{Endorsement, EndorsementDeserializerLW, WrappedEndorsement},
    error::ModelsError,
    operation::{OperationId, OperationIdsDeserializer, OperationIdsSerializer, WrappedOperation},
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U32VarIntDeserializer,
    U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::{KeyPair, PublicKey, Signature};
use nom::branch::alt;
use nom::bytes::complete::tag;
use nom::error::context;
use nom::multi::{count, length_count};
use nom::sequence::{preceded, tuple};
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
use std::convert::TryInto;
use std::fmt::Formatter;
use std::ops::Bound::{Excluded, Included};
use std::str::FromStr;

/// Size in bytes of a serialized block ID
const BLOCK_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// block id
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct BlockId(pub Hash);

impl PreHashed for BlockId {}

impl Id for BlockId {
    fn new(hash: Hash) -> Self {
        BlockId(hash)
    }

    fn get_hash(&self) -> &Hash {
        &self.0
    }
}

const BLOCKID_PREFIX: char = 'B';
const BLOCKID_VERSION: u64 = 0;

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&BLOCKID_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            BLOCKID_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl FromStr for BlockId {
    type Err = ModelsError;
    /// ## Example
    /// ```rust
    /// # use massa_hash::Hash;
    /// # use std::str::FromStr;
    /// # use massa_models::block::BlockId;
    /// # let hash = Hash::compute_from(b"test");
    /// # let block_id = BlockId(hash);
    /// let ser = block_id.to_string();
    /// let res_block_id = BlockId::from_str(&ser).unwrap();
    /// assert_eq!(block_id, res_block_id);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == BLOCKID_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::BlockIdParseError)?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::BlockIdParseError)?;
                Ok(BlockId(Hash::from_bytes(
                    rest.try_into()
                        .map_err(|_| ModelsError::BlockIdParseError)?,
                )))
            }
            _ => Err(ModelsError::BlockIdParseError),
        }
    }
}

impl BlockId {
    /// block id to bytes
    pub fn to_bytes(&self) -> &[u8; BLOCK_ID_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// block id into bytes
    pub fn into_bytes(self) -> [u8; BLOCK_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// block id from bytes
    pub fn from_bytes(data: &[u8; BLOCK_ID_SIZE_BYTES]) -> BlockId {
        BlockId(Hash::from_bytes(data))
    }

    /// first bit of the hashed block id
    pub fn get_first_bit(&self) -> bool {
        self.to_bytes()[0] >> 7 == 1
    }
}

/// Serializer for `BlockId`
#[derive(Default, Clone)]
pub struct BlockIdSerializer;

impl BlockIdSerializer {
    /// Creates a new serializer for `BlockId`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<BlockId> for BlockIdSerializer {
    fn serialize(&self, value: &BlockId, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `BlockId`
#[derive(Default, Clone)]
pub struct BlockIdDeserializer {
    hash_deserializer: HashDeserializer,
}

impl BlockIdDeserializer {
    /// Creates a new deserializer for `BlockId`
    pub fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<BlockId> for BlockIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockId, E> {
        context("Failed BlockId deserialization", |input| {
            let (rest, hash) = self.hash_deserializer.deserialize(input)?;
            Ok((rest, BlockId(hash)))
        })(buffer)
    }
}

/// block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// signed header
    pub header: WrappedHeader,
    /// operations
    pub operations: Vec<OperationId>,
}

/// Wrapped Block
pub type WrappedBlock = Wrapped<Block, BlockId>;

impl WrappedContent for Block {
    fn new_wrapped<SC: Serializer<Self>, U: Id>(
        content: Self,
        content_serializer: SC,
        _keypair: &KeyPair,
    ) -> Result<Wrapped<Self, U>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        Ok(Wrapped {
            signature: content.header.signature,
            creator_public_key: content.header.creator_public_key,
            creator_address: content.header.creator_address,
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
    ) -> IResult<&'a [u8], Wrapped<Self, U>, E> {
        let (rest, content) = content_deserializer.deserialize(buffer)?;
        Ok((
            rest,
            Wrapped {
                signature: content.header.signature,
                creator_public_key: content.header.creator_public_key,
                creator_address: content.header.creator_address,
                id: U::new(*content.header.id.get_hash()),
                content,
                serialized_data: buffer[..buffer.len() - rest.len()].to_vec(),
            },
        ))
    }
}
/// Serializer for `Block`
pub struct BlockSerializer {
    header_serializer: WrappedSerializer,
    op_ids_serializer: OperationIdsSerializer,
}

impl BlockSerializer {
    /// Creates a new `BlockSerializer`
    pub fn new() -> Self {
        BlockSerializer {
            header_serializer: WrappedSerializer::new(),
            op_ids_serializer: OperationIdsSerializer::new(),
        }
    }
}

impl Default for BlockSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Block> for BlockSerializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block::{Block, BlockSerializer, BlockId, BlockHeader, BlockHeaderSerializer}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, wrapped::WrappedContent, prehash::PreHashSet};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///     .map(|i| BlockId(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_wrapped(
    ///     BlockHeader {
    ///         slot: Slot::new(1, 1),
    ///         parents,
    ///         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///         endorsements: vec![
    ///             Endorsement::new_wrapped(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_wrapped(
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
    fn serialize(&self, value: &Block, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.header_serializer.serialize(&value.header, buffer)?;
        self.op_ids_serializer
            .serialize(&value.operations, buffer)?;
        Ok(())
    }
}

/// Deserializer for `Block`
pub struct BlockDeserializer {
    header_deserializer: WrappedDeserializer<BlockHeader, BlockHeaderDeserializer>,
    op_ids_deserializer: OperationIdsDeserializer,
}

impl BlockDeserializer {
    /// Creates a new `BlockDeserializer`
    pub fn new(thread_count: u8, max_operations_per_block: u32, endorsement_count: u32) -> Self {
        BlockDeserializer {
            header_deserializer: WrappedDeserializer::new(BlockHeaderDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            op_ids_deserializer: OperationIdsDeserializer::new(max_operations_per_block),
        }
    }
}

impl Deserializer<Block> for BlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block::{Block, BlockSerializer, BlockDeserializer, BlockId,BlockHeader, BlockHeaderSerializer}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, wrapped::WrappedContent, prehash::PreHashSet};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///     .map(|i| BlockId(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_wrapped(
    ///     BlockHeader {
    ///         slot: Slot::new(1, 1),
    ///         parents,
    ///         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///         endorsements: vec![
    ///             Endorsement::new_wrapped(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_wrapped(
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
    ) -> IResult<&'a [u8], Block, E> {
        context(
            "Failed Block deserialization",
            tuple((
                context("Failed header deserialization", |input| {
                    self.header_deserializer.deserialize(input)
                }),
                context("Failed operations deserialization", |input| {
                    self.op_ids_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(header, operations)| Block { header, operations })
        .parse(buffer)
    }
}

impl WrappedBlock {
    /// size in bytes of the whole block
    pub fn bytes_count(&self) -> u64 {
        self.serialized_data.len() as u64
    }

    /// true if given operation is included in the block
    pub fn contains_operation(&self, op: WrappedOperation) -> bool {
        self.content.operations.contains(&op.id)
    }

    /// returns the fitness of the block
    pub fn get_fitness(&self) -> u64 {
        self.content.header.get_fitness()
    }
}

impl std::fmt::Display for Block {
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

/// block header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    /// slot
    pub slot: Slot,
    /// parents
    pub parents: Vec<BlockId>,
    /// all operations hash
    pub operation_merkle_root: Hash,
    /// endorsements
    pub endorsements: Vec<WrappedEndorsement>,
}

// NOTE: TODO
// impl Signable<BlockId> for BlockHeader {
//     fn get_signature_message(&self) -> Result<Hash, ModelsError> {
//         let hash = self.compute_hash()?;
//         let mut res = [0u8; SLOT_KEY_SIZE + BLOCK_ID_SIZE_BYTES];
//         res[..SLOT_KEY_SIZE].copy_from_slice(&self.slot.to_bytes_key());
//         res[SLOT_KEY_SIZE..].copy_from_slice(hash.to_bytes());
//         // rehash for safety
//         Ok(Hash::compute_from(&res))
//     }
// }

/// wrapped header
pub type WrappedHeader = Wrapped<BlockHeader, BlockId>;

impl WrappedHeader {
    /// gets the header fitness
    pub fn get_fitness(&self) -> u64 {
        (self.content.endorsements.len() as u64) + 1
    }
}

impl WrappedContent for BlockHeader {}

/// Serializer for `BlockHeader`
pub struct BlockHeaderSerializer {
    slot_serializer: SlotSerializer,
    endorsement_serializer: WrappedSerializer,
    endorsement_content_serializer: EndorsementSerializerLW,
    u32_serializer: U32VarIntSerializer,
}

impl BlockHeaderSerializer {
    /// Creates a new `BlockHeaderSerializer`
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            endorsement_serializer: WrappedSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
            endorsement_content_serializer: EndorsementSerializerLW::new(),
        }
    }
}

impl Default for BlockHeaderSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BlockHeader> for BlockHeaderSerializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::block::{BlockId, BlockHeader, BlockHeaderSerializer};
    /// use massa_models::endorsement::{Endorsement, EndorsementSerializer};
    /// use massa_models::wrapped::WrappedContent;
    /// use massa_models::{config::THREAD_COUNT, slot::Slot};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::Serializer;
    ///
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///   .map(|i| BlockId(Hash::compute_from(&[i])))
    ///   .collect();
    /// let header = BlockHeader {
    ///   slot: Slot::new(1, 1),
    ///   parents,
    ///   operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///   endorsements: vec![
    ///     Endorsement::new_wrapped(
    ///        Endorsement {
    ///          slot: Slot::new(1, 1),
    ///          index: 1,
    ///          endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///        },
    ///     EndorsementSerializer::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///     Endorsement::new_wrapped(
    ///       Endorsement {
    ///         slot: Slot::new(4, 0),
    ///         index: 3,
    ///         endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    ///       },
    ///     EndorsementSerializer::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///    ],
    /// };
    /// let mut buffer = vec![];
    /// BlockHeaderSerializer::new().serialize(&header, &mut buffer).unwrap();
    /// ```
    fn serialize(&self, value: &BlockHeader, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.slot_serializer.serialize(&value.slot, buffer)?;
        // parents (note: there should be none if slot period=0)
        if value.parents.is_empty() {
            buffer.push(0);
        } else {
            buffer.push(1);
        }
        for parent_h in value.parents.iter() {
            buffer.extend(parent_h.0.to_bytes());
        }

        // operations merkle root
        buffer.extend(value.operation_merkle_root.to_bytes());

        self.u32_serializer.serialize(
            &value.endorsements.len().try_into().map_err(|err| {
                SerializeError::GeneralError(format!("too many endorsements: {}", err))
            })?,
            buffer,
        )?;
        for endorsement in value.endorsements.iter() {
            self.endorsement_serializer.serialize_with(
                &self.endorsement_content_serializer,
                endorsement,
                buffer,
            )?;
        }
        Ok(())
    }
}

/// Deserializer for `BlockHeader`
pub struct BlockHeaderDeserializer {
    slot_deserializer: SlotDeserializer,
    endorsement_serializer: EndorsementSerializer,
    length_endorsements_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    thread_count: u8,
    endorsement_count: u32,
}

impl BlockHeaderDeserializer {
    /// Creates a new `BlockHeaderDeserializerLW`
    pub const fn new(thread_count: u8, endorsement_count: u32) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            endorsement_serializer: EndorsementSerializer::new(),
            length_endorsements_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(endorsement_count),
            ),
            hash_deserializer: HashDeserializer::new(),
            thread_count,
            endorsement_count,
        }
    }
}

impl Deserializer<BlockHeader> for BlockHeaderDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::block::{BlockId, BlockHeader, BlockHeaderDeserializer, BlockHeaderSerializer};
    /// use massa_models::{config::THREAD_COUNT, slot::Slot, wrapped::WrappedContent};
    /// use massa_models::endorsement::{Endorsement, EndorsementSerializerLW};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let keypair = KeyPair::generate();
    /// let parents = (0..THREAD_COUNT)
    ///   .map(|i| BlockId(Hash::compute_from(&[i])))
    ///   .collect();
    /// let header = BlockHeader {
    ///   slot: Slot::new(1, 1),
    ///   parents,
    ///   operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///   endorsements: vec![
    ///     Endorsement::new_wrapped(
    ///        Endorsement {
    ///          slot: Slot::new(1, 1),
    ///          index: 1,
    ///          endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
    ///        },
    ///     EndorsementSerializerLW::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///     Endorsement::new_wrapped(
    ///       Endorsement {
    ///         slot: Slot::new(4, 0),
    ///         index: 3,
    ///         endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
    ///       },
    ///     EndorsementSerializerLW::new(),
    ///     &keypair,
    ///     )
    ///     .unwrap(),
    ///    ],
    /// };
    /// let mut buffer = vec![];
    /// BlockHeaderSerializer::new().serialize(&header, &mut buffer).unwrap();
    /// let (rest, deserialized_header) = BlockHeaderDeserializer::new(32, 9).deserialize::<DeserializeError>(&buffer).unwrap();
    /// assert_eq!(rest.len(), 0);
    /// let mut buffer2 = Vec::new();
    /// BlockHeaderSerializer::new().serialize(&deserialized_header, &mut buffer2).unwrap();
    /// assert_eq!(buffer, buffer2);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockHeader, E> {
        let (rest, (slot, parents, operation_merkle_root)): (&[u8], (Slot, Vec<BlockId>, Hash)) =
            context(
                "Failed BlockHeader deserialization",
                tuple((
                    context("Failed slot deserialization", |input| {
                        self.slot_deserializer.deserialize(input)
                    }),
                    context(
                        "Failed parents deserialization",
                        alt((
                            preceded(tag(&[0]), |input| Ok((input, Vec::new()))),
                            preceded(
                                tag(&[1]),
                                count(
                                    context("Failed block_id deserialization", |input| {
                                        self.hash_deserializer
                                            .deserialize(input)
                                            .map(|(rest, hash)| (rest, BlockId(hash)))
                                    }),
                                    self.thread_count as usize,
                                ),
                            ),
                        )),
                    ),
                    context("Failed operation_merkle_root", |input| {
                        self.hash_deserializer.deserialize(input)
                    }),
                )),
            )
            .parse(buffer)?;

        if parents.is_empty() {
            return Ok((
                &rest[1..], // Because there is 0 endorsements, we have a remaining 0 in rest and we don't need it
                BlockHeader {
                    slot,
                    parents,
                    operation_merkle_root,
                    endorsements: Vec::new(),
                },
            ));
        }
        // Now deser the endorsements (which were: lw serialized)
        let endorsement_deserializer = WrappedDeserializer::new(EndorsementDeserializerLW::new(
            self.endorsement_count,
            slot,
            parents[slot.thread as usize],
        ));

        let (rest, endorsements) = context(
            "Failed endorsements deserialization",
            length_count::<&[u8], Wrapped<Endorsement, EndorsementId>, u32, E, _, _>(
                context("Failed length deserialization", |input| {
                    self.length_endorsements_deserializer.deserialize(input)
                }),
                context("Failed endorsement deserialization", |input| {
                    endorsement_deserializer.deserialize_with(&self.endorsement_serializer, input)
                }),
            ),
        )
        .parse(rest)?;

        Ok((
            rest,
            BlockHeader {
                slot,
                parents,
                operation_merkle_root,
                endorsements,
            },
        ))
    }
}

impl std::fmt::Display for BlockHeader {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\t(period: {}, thread: {})",
            self.slot.period, self.slot.thread,
        )?;
        writeln!(f, "\tMerkle root: {}", self.operation_merkle_root,)?;
        writeln!(f, "\tParents: ")?;
        for id in self.parents.iter() {
            let str_id = id.to_string();
            writeln!(f, "\t\t{}", str_id)?;
        }
        if self.parents.is_empty() {
            writeln!(f, "No parents found: This is a genesis header")?;
        }
        writeln!(f, "\tEndorsements:")?;
        for ed in self.endorsements.iter() {
            writeln!(f, "\t\t-----")?;
            writeln!(f, "\t\tId: {}", ed.id)?;
            writeln!(f, "\t\tIndex: {}", ed.content.index)?;
            writeln!(f, "\t\tEndorsed slot: {}", ed.content.slot)?;
            writeln!(f, "\t\tEndorser's public key: {}", ed.creator_public_key)?;
            writeln!(f, "\t\tEndorsed block: {}", ed.content.endorsed_block)?;
            writeln!(f, "\t\tSignature: {}", ed.signature)?;
        }
        if self.endorsements.is_empty() {
            writeln!(f, "\tNo endorsements found")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        config::{ENDORSEMENT_COUNT, MAX_OPERATIONS_PER_BLOCK, THREAD_COUNT},
        endorsement::Endorsement,
        endorsement::EndorsementSerializer,
    };
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_block_serialization() {
        let keypair =
            KeyPair::from_str("S1bXjyPwrssNmG4oUG5SEqaUhQkVArQi7rzQDWpCprTSmEgZDGG").unwrap();
        let parents = (0..THREAD_COUNT)
            .map(|_i| {
                BlockId(
                    Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf")
                        .unwrap(),
                )
            })
            .collect();

        let endo = Endorsement::new_wrapped(
            Endorsement {
                slot: Slot::new(1, 0),
                index: 1,
                endorsed_block: BlockId(
                    Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf")
                        .unwrap(),
                ),
            },
            EndorsementSerializer::new(),
            &keypair,
        )
        .unwrap();

        // create block header
        let orig_header = BlockHeader::new_wrapped(
            BlockHeader {
                slot: Slot::new(1, 0),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![endo],
            },
            BlockHeaderSerializer::new(),
            &keypair,
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header.clone(),
            operations: Default::default(),
        };

        // serialize block
        let wrapped_block: WrappedBlock =
            Block::new_wrapped(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        WrappedSerializer::new()
            .serialize(&wrapped_block, &mut ser_block)
            .unwrap();

        // deserialize
        let (rest, res_block): (&[u8], WrappedBlock) = WrappedDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block)
        .unwrap();

        assert!(rest.is_empty());
        // check equality
        assert_eq!(orig_block.header.id, res_block.content.header.id);
        assert_eq!(orig_block.header.id, res_block.id);
        assert_eq!(
            orig_block.header.content.slot,
            res_block.content.header.content.slot
        );
        assert_eq!(
            orig_block.header.serialized_data,
            res_block.content.header.serialized_data
        );
        assert_eq!(
            orig_block.header.signature,
            res_block.content.header.signature
        );

        assert_eq!(
            orig_block.header.content.endorsements,
            res_block.content.header.content.endorsements
        );

        assert_eq!(orig_block.header.signature, res_block.signature);
        orig_header.verify_signature().unwrap();
        for ed in orig_block.header.content.endorsements.iter() {
            ed.verify_signature().unwrap();
        }
        res_block.content.header.verify_signature().unwrap();
        for ed in res_block.content.header.content.endorsements.iter() {
            ed.verify_signature().unwrap();
        }
    }

    #[test]
    #[serial]
    fn test_genesis_block_serialization() {
        let keypair = KeyPair::generate();
        let parents: Vec<BlockId> = vec![];

        // create block header
        let orig_header = BlockHeader::new_wrapped(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![],
            },
            BlockHeaderSerializer::new(),
            &keypair,
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header,
            operations: Default::default(),
        };

        // serialize block
        let wrapped_block: WrappedBlock =
            Block::new_wrapped(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        WrappedSerializer::new()
            .serialize(&wrapped_block, &mut ser_block)
            .unwrap();

        // deserialize
        let (rest, res_block): (&[u8], WrappedBlock) = WrappedDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block)
        .unwrap();

        // check equality

        assert!(rest.is_empty());
        assert_eq!(orig_block.header.id, res_block.content.header.id);
        // assert_eq!(orig_block.header.id, res_block.id);
        assert_eq!(
            orig_block.header.content.slot,
            res_block.content.header.content.slot
        );
        assert_eq!(
            orig_block.header.serialized_data,
            res_block.content.header.serialized_data
        );
        assert_eq!(
            orig_block.header.signature,
            res_block.content.header.signature
        );

        assert_eq!(
            orig_block.header.content.endorsements,
            res_block.content.header.content.endorsements
        );

        assert_eq!(orig_block.header.signature, res_block.signature);
    }

    #[test]
    #[serial]
    fn test_invalid_genesis_block_serialization() {
        let keypair = KeyPair::generate();
        let parents: Vec<BlockId> = vec![];

        // Genesis block do not have any parents and thus cannot embed endorsements
        let endorsement = Endorsement {
            slot: Slot::new(1, 1),
            index: 1,
            endorsed_block: BlockId(Hash::compute_from(&[1])),
        };

        // create block header
        let orig_header = BlockHeader::new_wrapped(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![Endorsement::new_wrapped(
                    endorsement,
                    EndorsementSerializer::new(),
                    &keypair,
                )
                .unwrap()],
            },
            BlockHeaderSerializer::new(),
            &keypair,
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header,
            operations: Default::default(),
        };

        // serialize block
        let wrapped_block: WrappedBlock =
            Block::new_wrapped(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        WrappedSerializer::new()
            .serialize(&wrapped_block, &mut ser_block)
            .unwrap();

        // deserialize
        let res: Result<(&[u8], WrappedBlock), _> = WrappedDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block);

        assert!(res.is_err());
    }
}
