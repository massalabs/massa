// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::constants::{BLOCK_ID_SIZE_BYTES, SLOT_KEY_SIZE};
use crate::endorsement::{WrappedEndorsementDeserializer, WrappedEndorsementSerializer};
use crate::node_configuration::{MAX_BLOCK_SIZE, MAX_OPERATIONS_PER_BLOCK, THREAD_COUNT};
use crate::operation::{
    OperationDeserializer, WrappedOperationDeserializer, WrappedOperationSerializer,
};
use crate::prehash::{Map, PreHashed, Set};
use crate::wrapped::{Id, Wrapped, WrappedDeserializer, WrappedSerializer};
use crate::{
    array_from_slice, u8_from_slice, with_serialization_context, Address, DeserializeCompact,
    DeserializeMinBEInt, DeserializeVarInt, Endorsement, EndorsementDeserializer, EndorsementId,
    ModelsError, Operation, OperationId, SerializeCompact, SerializeMinBEInt, SerializeVarInt,
    Slot, SlotDeserializer, SlotSerializer, WrappedEndorsement, WrappedOperation,
};
use massa_hash::HASH_SIZE_BYTES;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_signature::{PublicKey, PUBLIC_KEY_SIZE_BYTES};
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
use std::ops::Bound::Included;
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
    pub operations: Vec<WrappedOperation>,
}

pub type WrappedBlock = Wrapped<Block, BlockId>;
pub type WrappedBlockSerializer = WrappedSerializer<Block, BlockId>;
pub type WrappedBlockDeserializer = WrappedDeserializer<Block, BlockId, BlockDeserializer>;

pub struct BlockSerializer {
    header_serializer: WrappedHeaderSerializer,
    operation_serializer: WrappedOperationSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl BlockSerializer {
    pub fn new() -> Self {
        BlockSerializer {
            header_serializer: WrappedHeaderSerializer::new(),
            operation_serializer: WrappedOperationSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(
                Included(0),
                Included(MAX_OPERATIONS_PER_BLOCK),
            ),
        }
    }
}

impl Default for BlockSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<Block> for BlockSerializer {
    fn serialize(&self, value: &Block, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.header_serializer.serialize(&value.header, buffer)?;
        self.u32_serializer.serialize(
            &value.operations.len().try_into().map_err(|err| {
                SerializeError::NumberTooBig(format!("too many operations: {}", err))
            })?,
            buffer,
        )?;
        for operation in value.operations.iter() {
            self.operation_serializer.serialize(operation, buffer)?;
        }
        Ok(())
    }
}

pub struct BlockDeserializer {
    header_deserializer: WrappedHeaderDeserializer,
    operation_deserializer: WrappedOperationDeserializer,
    u32_deserializer: U32VarIntDeserializer,
}

impl BlockDeserializer {
    pub fn new() -> Self {
        BlockDeserializer {
            header_deserializer: WrappedHeaderDeserializer::new(BlockHeaderDeserializer::new()),
            operation_deserializer: WrappedOperationDeserializer::new(OperationDeserializer::new()),
            u32_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(MAX_OPERATIONS_PER_BLOCK),
            ),
        }
    }
}

impl Default for BlockDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<Block> for BlockDeserializer {
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
                length_count(
                    context("Failed length operation deserialization", |input| {
                        self.u32_deserializer.deserialize(input)
                    }),
                    context("Failed operation deserialization", |input| {
                        let (rest, operation) = self.operation_deserializer.deserialize(input)?;
                        if buffer.len() - rest.len() > MAX_BLOCK_SIZE as usize {
                            return Err(nom::Err::Error(ParseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::TooLarge,
                            )));
                        }
                        Ok((rest, operation))
                    }),
                ),
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
}

impl Block {
    /// true if given operation is included in the block
    /// may fail if computing an id of an operation in the block
    pub fn contains_operation(&self, op: WrappedOperation) -> Result<bool, ModelsError> {
        let op_id = op.id;
        Ok(self.operations.iter().any(|o| op_id == o.id))
    }

    /// Retrieve roll involving addresses
    pub fn get_roll_involved_addresses(&self) -> Result<Set<Address>, ModelsError> {
        let mut roll_involved_addrs = Set::<Address>::default();
        for op in self.operations.iter() {
            roll_involved_addrs.extend(op.get_roll_involved_addresses()?);
        }
        Ok(roll_involved_addrs)
    }

    /// retrieves a mapping of addresses to the list of operation IDs they are involved with in terms of ledger
    pub fn involved_addresses(
        &self,
        operation_set: &Map<OperationId, (usize, u64)>,
    ) -> Result<Map<Address, Set<OperationId>>, ModelsError> {
        let mut addresses_to_operations: Map<Address, Set<OperationId>> =
            Map::<Address, Set<OperationId>>::default();
        operation_set
            .iter()
            .try_for_each::<_, Result<(), ModelsError>>(|(op_id, (op_idx, _op_expiry))| {
                let op = &self.operations[*op_idx];
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
        self.header
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

/// signed header
pub type WrappedHeader = Wrapped<BlockHeader, BlockId>;
pub type WrappedHeaderSerializer = WrappedSerializer<BlockHeader, BlockId>;
pub type WrappedHeaderDeserializer =
    WrappedDeserializer<BlockHeader, BlockId, BlockHeaderDeserializer>;

pub struct BlockHeaderSerializer {
    slot_serializer: SlotSerializer,
    endorsement_serializer: WrappedEndorsementSerializer,
    u32_serializer: U32VarIntSerializer,
}

impl BlockHeaderSerializer {
    pub fn new() -> Self {
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        Self {
            slot_serializer: SlotSerializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Included(thread_count)),
            ),
            endorsement_serializer: WrappedEndorsementSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(Included(0), Included(u32::MAX)),
        }
    }
}

impl Default for BlockHeaderSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<BlockHeader> for BlockHeaderSerializer {
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

pub struct BlockHeaderDeserializer {
    slot_deserializer: SlotDeserializer,
    endorsement_deserializer: WrappedEndorsementDeserializer,
    u32_deserializer: U32VarIntDeserializer,
    hash_deserializer: HashDeserializer,
}

impl BlockHeaderDeserializer {
    pub fn new() -> Self {
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Included(thread_count)),
            ),
            endorsement_deserializer: WrappedEndorsementDeserializer::new(
                EndorsementDeserializer::new(),
            ),
            u32_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Default for BlockHeaderDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<BlockHeader> for BlockHeaderDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockHeader, E> {
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
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
                                |input| {
                                    self.hash_deserializer
                                        .deserialize(input)
                                        .map(|(rest, hash)| (rest, BlockId(hash)))
                                },
                                thread_count as usize,
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
                        |input| self.u32_deserializer.deserialize(input),
                        |input| self.endorsement_deserializer.deserialize(input),
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
    use crate::{endorsement::EndorsementSerializer, Endorsement};
    use massa_serialization::DeserializeError;
    use massa_signature::{derive_public_key, generate_random_private_key};
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_block_serialization() {
        let ctx = crate::SerializationContext {
            max_block_size: 1024 * 1024,
            max_operations_per_block: 1024,
            thread_count: 3,
            max_advertise_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_bootstrap_pos_cycles: 1000,
            max_bootstrap_pos_entries: 1000,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_bootstrap_message_size: 100000000,
            endorsement_count: 8,
        };
        crate::init_serialization_context(ctx);
        let private_key = generate_random_private_key();
        let public_key = derive_public_key(&private_key);

        // create block header
        let orig_header = Wrapped::new_wrapped(
            BlockHeader {
                slot: Slot::new(1, 2),
                parents: vec![
                    BlockId(Hash::compute_from("abc".as_bytes())),
                    BlockId(Hash::compute_from("def".as_bytes())),
                    BlockId(Hash::compute_from("ghi".as_bytes())),
                ],
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![
                    Wrapped::new_wrapped(
                        Endorsement {
                            slot: Slot::new(1, 1),
                            index: 1,
                            endorsed_block: BlockId(Hash::compute_from("blk1".as_bytes())),
                        },
                        EndorsementSerializer::new(),
                        &private_key,
                    )
                    .unwrap(),
                    Wrapped::new_wrapped(
                        Endorsement {
                            slot: Slot::new(4, 0),
                            index: 3,
                            endorsed_block: BlockId(Hash::compute_from("blk2".as_bytes())),
                        },
                        EndorsementSerializer::new(),
                        &private_key,
                    )
                    .unwrap(),
                ],
            },
            BlockHeaderSerializer::new(),
            &private_key,
        )
        .unwrap();

        // create block
        let orig_block = Block {
            header: orig_header,
            operations: vec![],
        };

        // serialize block
        let mut orig_bytes = Vec::new();
        BlockSerializer::new()
            .serialize(&orig_block, &mut orig_bytes)
            .unwrap();

        // deserialize
        let (rest, res_block) = BlockDeserializer::new()
            .deserialize::<DeserializeError>(&orig_bytes)
            .unwrap();
        assert!(rest.is_empty());

        // check equality
        // TODO: AURELIEN UNCOMMENT
        //let generated_res_id = res_block.header.content.compute_id().unwrap();
        //assert_eq!(orig_id, res_id);
        //assert_eq!(orig_id, generated_res_id);
        assert_eq!(res_block.header.signature, orig_block.header.signature);
    }
}
