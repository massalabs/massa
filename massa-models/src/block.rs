// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::BLOCK_ID_SIZE_BYTES;
use crate::node_configuration::THREAD_COUNT;
use crate::prehash::{Map, PreHashed, Set};
use crate::wrapped::{Id, Wrapped, WrappedContent, WrappedDeserializer, WrappedSerializer};
use crate::{
    Address, Endorsement, EndorsementDeserializer, EndorsementId, ModelsError, OperationId,
    OperationIds, OperationIdsDeserializer, OperationIdsSerializer, Slot, SlotDeserializer,
    SlotSerializer, WrappedEndorsement, WrappedOperation,
};
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
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

const BLOCK_ID_STRING_PREFIX: &str = "BLO";

/// block id
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct BlockId(pub Hash);

impl PreHashed for BlockId {}

impl Id for BlockId {
    fn new(hash: Hash) -> Self {
        BlockId(hash)
    }

    fn hash(&self) -> Hash {
        self.0
    }
}

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", BLOCK_ID_STRING_PREFIX, self.0.to_bs58_check())
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl std::fmt::Debug for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if cfg!(feature = "hash-prefix") {
            write!(f, "{}-{}", BLOCK_ID_STRING_PREFIX, self.0.to_bs58_check())
        } else {
            write!(f, "{}", self.0.to_bs58_check())
        }
    }
}

impl FromStr for BlockId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if cfg!(feature = "hash-prefix") {
            let v: Vec<_> = s.split('-').collect();
            if v.len() != 2 {
                // assume there is no prefix
                Ok(BlockId(Hash::from_str(s)?))
            } else if v[0] != BLOCK_ID_STRING_PREFIX {
                Err(ModelsError::WrongPrefix(
                    BLOCK_ID_STRING_PREFIX.to_string(),
                    v[0].to_string(),
                ))
            } else {
                Ok(BlockId(Hash::from_str(v[1])?))
            }
        } else {
            Ok(BlockId(Hash::from_str(s)?))
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

    /// block id fro `bs58` check
    pub fn from_bs58_check(data: &str) -> Result<BlockId, ModelsError> {
        Ok(BlockId(
            Hash::from_bs58_check(data).map_err(|_| ModelsError::HashError)?,
        ))
    }

    /// first bit of the hashed block id
    pub fn get_first_bit(&self) -> bool {
        self.to_bytes()[0] >> 7 == 1
    }
}

/// block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// signed header
    pub header: WrappedHeader,
    /// operations
    pub operations: OperationIds,
}

/// Wrapped Block
pub type WrappedBlock = Wrapped<Block, BlockId>;

impl WrappedContent for Block {
    fn new_wrapped<SC: Serializer<Self>, U: Id>(
        content: Self,
        content_serializer: SC,
        keypair: &KeyPair,
    ) -> Result<Wrapped<Self, U>, ModelsError> {
        let public_key = keypair.get_public_key();
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        let creator_address = Address::from_public_key(&public_key);

        Ok(Wrapped {
            signature: content.header.signature,
            creator_public_key: public_key,
            creator_address,
            thread: creator_address.get_thread(THREAD_COUNT),
            id: U::new(content.header.id.hash()),
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
                thread: content.header.thread,
                id: U::new(content.header.id.hash()),
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
    /// use massa_models::{Block, BlockSerializer, BlockId, constants::THREAD_COUNT, Slot, BlockHeader, BlockHeaderSerializer, Endorsement, EndorsementSerializer, wrapped::WrappedContent};
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
    ///     operations: vec![],
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
    /// use massa_models::{Block, BlockSerializer, BlockDeserializer, BlockId, constants::THREAD_COUNT, Slot, BlockHeader, BlockHeaderSerializer, Endorsement, EndorsementSerializer, wrapped::WrappedContent};
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
    ///     operations: vec![],
    /// };
    ///
    /// let mut buffer = Vec::new();
    /// BlockSerializer::new().serialize(&orig_block, &mut buffer).unwrap();
    /// let (rest, res_block) = BlockDeserializer::new(THREAD_COUNT, 100, 9, 10000, 10000, 10000).deserialize::<DeserializeError>(&mut buffer).unwrap();
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
    /// may fail if computing an id of an operation in the block
    pub fn contains_operation(&self, op: WrappedOperation) -> Result<bool, ModelsError> {
        Ok(self.content.operations.contains(&op.id))
    }

    /// retrieves a mapping of addresses to the list of operation IDs they are involved with in terms of ledger
    pub fn involved_addresses(
        &self,
        operation_set: &Map<OperationId, WrappedOperation>,
    ) -> Result<Map<Address, Set<OperationId>>, ModelsError> {
        let mut addresses_to_operations: Map<Address, Set<OperationId>> =
            Map::<Address, Set<OperationId>>::default();
        operation_set
            .iter()
            .try_for_each::<_, Result<(), ModelsError>>(|(op_id, op)| {
                let addrs = op.get_ledger_involved_addresses();
                for ad in addrs.into_iter() {
                    if let Some(entry) = addresses_to_operations.get_mut(&ad) {
                        entry.insert(*op_id);
                    } else {
                        let mut set = Set::<OperationId>::default();
                        set.insert(*op_id);
                        addresses_to_operations.insert(ad, set);
                    }
                }
                Ok(())
            })?;
        Ok(addresses_to_operations)
    }

    /// returns the set of addresses mapped the the endorsements they are involved in
    pub fn addresses_to_endorsements(
        &self,
    ) -> Result<Map<Address, Set<EndorsementId>>, ModelsError> {
        let mut res: Map<Address, Set<EndorsementId>> = Map::default();
        self.content
            .header
            .content
            .endorsements
            .iter()
            .try_for_each::<_, Result<(), ModelsError>>(|e| {
                let address = Address::from_public_key(&e.creator_public_key);
                if let Some(old) = res.get_mut(&address) {
                    old.insert(e.id);
                } else {
                    let mut set = Set::<EndorsementId>::default();
                    set.insert(e.id);
                    res.insert(address, set);
                }
                Ok(())
            })?;
        Ok(res)
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

impl WrappedContent for BlockHeader {}

/// Serializer for `BlockHeader`
pub struct BlockHeaderSerializer {
    slot_serializer: SlotSerializer,
    endorsement_serializer: WrappedSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl BlockHeaderSerializer {
    /// Creates a new `BlockHeaderSerializer`
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            endorsement_serializer: WrappedSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
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
    /// use massa_models::{BlockId, constants::THREAD_COUNT, Slot, BlockHeader, BlockHeaderSerializer, Endorsement, EndorsementSerializer, wrapped::WrappedContent};
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
            self.endorsement_serializer.serialize(endorsement, buffer)?;
        }
        Ok(())
    }
}

/// Deserializer for `BlockHeader`
pub struct BlockHeaderDeserializer {
    slot_deserializer: SlotDeserializer,
    endorsement_deserializer: WrappedDeserializer<Endorsement, EndorsementDeserializer>,
    length_endorsements_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl BlockHeaderDeserializer {
    /// Creates a new `BlockHeaderDeserializer`
    pub const fn new(thread_count: u8, endorsement_count: u32) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            endorsement_deserializer: WrappedDeserializer::new(EndorsementDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            length_endorsements_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(endorsement_count),
            ),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<BlockHeader> for BlockHeaderDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{BlockId, constants::THREAD_COUNT, Slot, BlockHeader, BlockHeaderDeserializer, BlockHeaderSerializer, Endorsement, EndorsementSerializer, wrapped::WrappedContent};
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
                                THREAD_COUNT as usize,
                            ),
                        ),
                    )),
                ),
                context("Failed operation_merkle_root", |input| {
                    self.hash_deserializer.deserialize(input)
                }),
                context(
                    "Failed endorsements deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.length_endorsements_deserializer.deserialize(input)
                        }),
                        context("Failed endorsement deserialization", |input| {
                            self.endorsement_deserializer.deserialize(input)
                        }),
                    ),
                ),
            )),
        )
        .map(
            |(slot, parents, operation_merkle_root, endorsements)| BlockHeader {
                slot,
                parents,
                operation_merkle_root,
                endorsements,
            },
        )
        .parse(buffer)
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
        endorsement::EndorsementSerializer,
        node_configuration::{ENDORSEMENT_COUNT, MAX_OPERATIONS_PER_BLOCK},
        Endorsement,
    };
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_block_serialization() {
        let keypair = KeyPair::generate();
        let parents = (0..THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i])))
            .collect();

        // create block header
        let orig_header = BlockHeader::new_wrapped(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![
                    Endorsement::new_wrapped(
                        Endorsement {
                            slot: Slot::new(1, 1),
                            index: 1,
                            endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                        },
                        EndorsementSerializer::new(),
                        &keypair,
                    )
                    .unwrap(),
                    Endorsement::new_wrapped(
                        Endorsement {
                            slot: Slot::new(4, 0),
                            index: 3,
                            endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
                        },
                        EndorsementSerializer::new(),
                        &keypair,
                    )
                    .unwrap(),
                ],
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
        assert_eq!(orig_block.header.signature, res_block.signature);
    }
}
