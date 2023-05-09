//! Copyright (c) 2022 MASSA LABS <info@massa.net>

// use crate::config::THREAD_COUNT;
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
use massa_serialization::{
    // DeserializeError,
    Deserializer,
    SerializeError,
    Serializer,
    // U32VarIntDeserializer,
    // U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::{KeyPair, PublicKey, Signature};
// use nom::branch::alt;
// use nom::bytes::complete::tag;
use nom::error::context;
// use nom::multi::{count, length_count};
use nom::sequence::tuple;
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
// use serde_with::{DeserializeFromStr, SerializeDisplay};
// use std::collections::HashSet;
// use std::convert::TryInto;
use std::fmt::Formatter;
// use std::ops::Bound::{Excluded, Included};
// use std::str::FromStr;
use crate::block_header::{BlockHeader, BlockHeaderDeserializer, SecuredHeader};
use crate::block_id::BlockId;

/// block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    /// signed header
    pub header: SecuredHeader,
    /// operations ids
    pub operations: Vec<OperationId>,
}

/// filled block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilledBlock {
    /// signed header
    pub(crate) header: SecuredHeader,
    /// operations
    pub(crate) operations: Vec<(OperationId, Option<SecureShareOperation>)>,
}

/// Block with assosciated meta-data and interfaces allowing trust of data in untrusted network
pub type SecureShareBlock = SecureShare<Block, BlockId>;

impl SecureShareContent for Block {
    fn new_verifiable<SC: Serializer<Self>, U: Id>(
        self,
        content_serializer: SC,
        _keypair: &KeyPair,
    ) -> Result<SecureShare<Self, U>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&self, &mut content_serialized)?;
        Ok(SecureShare {
            signature: self.header.signature,
            content_creator_pub_key: self.header.content_creator_pub_key,
            content_creator_address: self.header.content_creator_address,
            id: U::new(*self.header.id.get_hash()),
            content: self,
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
pub(crate) struct BlockSerializer {
    header_serializer: SecureShareSerializer,
    op_ids_serializer: OperationIdsSerializer,
}

impl BlockSerializer {
    /// Creates a new `BlockSerializer`
    pub(crate) fn new() -> Self {
        BlockSerializer {
            header_serializer: SecureShareSerializer::new(),
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
    /// use massa_models::{block::{Block, BlockSerializer}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, secure_share::SecureShareContent, prehash::PreHashSet};
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_models::block_id::{BlockId};
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
    ///         denunciations: Vec::new(),
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

/// Parameters for the deserializer of a block
pub struct BlockDeserializerArgs {
    /// Number of threads in Massa
    pub thread_count: u8,
    /// Maximum of operations in a block
    pub(crate) max_operations_per_block: u32,
    /// Number of endorsements in a block
    pub(crate) endorsement_count: u32,
    /// Max denunciations in a block
    pub(crate) max_denunciations_per_block_header: u32,
    /// If Some(lsp), this will through if trying to deserialize a block with a period before the genesis blocks
    pub(crate) last_start_period: Option<u64>,
}

/// Deserializer for `Block`
pub struct BlockDeserializer {
    header_deserializer: SecureShareDeserializer<BlockHeader, BlockHeaderDeserializer>,
    op_ids_deserializer: OperationIdsDeserializer,
}

impl BlockDeserializer {
    /// Creates a new `BlockDeserializer`
    pub fn new(args: BlockDeserializerArgs) -> Self {
        BlockDeserializer {
            header_deserializer: SecureShareDeserializer::new(BlockHeaderDeserializer::new(
                args.thread_count,
                args.endorsement_count,
                args.max_denunciations_per_block_header,
                args.last_start_period,
            )),
            op_ids_deserializer: OperationIdsDeserializer::new(args.max_operations_per_block),
        }
    }
}

impl Deserializer<Block> for BlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block::{Block, BlockSerializer, BlockDeserializer, BlockDeserializerArgs}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, secure_share::SecureShareContent, prehash::PreHashSet};
    /// use massa_models::block_id::BlockId;
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_hash::Hash;
    /// use massa_signature::KeyPair;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// let keypair = KeyPair::generate();
    /// let parents: Vec<BlockId> = (0..THREAD_COUNT)
    ///     .map(|i| BlockId(Hash::compute_from(&[i])))
    ///     .collect();
    ///
    /// // create block header
    /// let orig_header = BlockHeader::new_verifiable(
    ///     BlockHeader {
    ///         slot: Slot::new(1, 1),
    ///         parents: parents.clone(),
    ///         operation_merkle_root: Hash::compute_from("mno".as_bytes()),
    ///         endorsements: vec![
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 0,
    ///                     endorsed_block: parents[1].clone(),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///             Endorsement::new_verifiable(
    ///                 Endorsement {
    ///                     slot: Slot::new(1, 1),
    ///                     index: 1,
    ///                     endorsed_block: parents[1].clone(),
    ///                 },
    ///                 EndorsementSerializer::new(),
    ///                 &keypair,
    ///             )
    ///             .unwrap(),
    ///         ],
    ///         denunciations: Vec::new(),
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
    /// let args = BlockDeserializerArgs {
    ///     thread_count: THREAD_COUNT,
    ///     max_operations_per_block: 100,
    ///     endorsement_count: 9,
    ///     max_denunciations_per_block_header: 10,
    ///     last_start_period: Some(0)
    /// };
    /// let (rest, res_block) = BlockDeserializer::new(args).deserialize::<DeserializeError>(&mut buffer).unwrap();
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

impl SecureShareBlock {
    /// size in bytes of the whole block
    pub(crate) fn bytes_count(&self) -> u64 {
        self.serialized_data.len() as u64
    }

    /// true if given operation is included in the block
    pub(crate) fn contains_operation(&self, op: SecureShareOperation) -> bool {
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

/// Block status within the graph
#[derive(Eq, PartialEq, Debug, Deserialize, Serialize)]
pub enum BlockGraphStatus {
    /// received but not yet graph-processed
    Incoming,
    /// waiting for its slot
    WaitingForSlot,
    /// waiting for a missing dependency
    WaitingForDependencies,
    /// active in alternative cliques
    ActiveInAlternativeCliques,
    /// active in blockclique
    ActiveInBlockclique,
    /// forever applies
    Final,
    /// discarded for any reason
    Discarded,
    /// not found in graph
    NotFound,
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::config::MAX_DENUNCIATIONS_PER_BLOCK_HEADER;
    use crate::{
        block_header::BlockHeaderSerializer,
        config::{ENDORSEMENT_COUNT, MAX_OPERATIONS_PER_BLOCK, THREAD_COUNT},
        endorsement::Endorsement,
        endorsement::EndorsementSerializer,
        slot::Slot,
    };
    use massa_hash::Hash;
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use serial_test::serial;
    use std::str::FromStr;

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

        let endo1 = Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: BlockId(
                    Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf")
                        .unwrap(),
                ),
            },
            EndorsementSerializer::new(),
            &keypair,
        )
        .unwrap();
        let endo2 = Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 0),
                index: ENDORSEMENT_COUNT - 1,
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
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 0),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![endo1, endo2],
                denunciations: Vec::new(), // FIXME
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let (rest, res_block): (&[u8], SecureShareBlock) =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
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

        res_block
            .content
            .header
            .assert_invariants(THREAD_COUNT, ENDORSEMENT_COUNT)
            .unwrap();
    }

    #[test]
    #[serial]
    fn test_genesis_block_serialization() {
        let keypair = KeyPair::generate();
        let parents: Vec<BlockId> = vec![];

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(0, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let (rest, res_block): (&[u8], SecureShareBlock) =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block)
                .unwrap();

        res_block
            .content
            .header
            .assert_invariants(THREAD_COUNT, ENDORSEMENT_COUNT)
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
    fn test_invalid_genesis_block_serialization_with_endorsements() {
        let keypair = KeyPair::generate();
        let parents: Vec<BlockId> = vec![];

        // Genesis block do not have any parents and thus cannot embed endorsements
        let endorsement = Endorsement {
            slot: Slot::new(0, 1),
            index: 1,
            endorsed_block: BlockId(Hash::compute_from(&[1])),
        };

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(0, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![Endorsement::new_verifiable(
                    endorsement,
                    EndorsementSerializer::new(),
                    &keypair,
                )
                .unwrap()],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
        // let nom::Err::Failure(_) = res.unwrap_err() else {
        //     panic!("Deserialisation with invalid endorsements should be total fail");
        // };
    }
    #[test]
    #[serial]
    fn test_invalid_genesis_block_serialization_with_parents() {
        let keypair = KeyPair::generate();
        let parents = (0..THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i])))
            .collect();

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(0, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
    }
    #[test]
    #[serial]
    fn test_invalid_block_serialization_no_parents() {
        let keypair = KeyPair::generate();
        // Non genesis block must have THREAD_COUNT parents

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents: vec![],
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
    }
    #[test]
    #[serial]
    fn test_invalid_block_serialization_obo_high_parent_count() {
        let keypair = KeyPair::generate();
        // Non genesis block must have THREAD_COUNT parents
        let parents = (0..=THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i])))
            .collect();

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
    }

    #[test]
    #[serial]
    fn test_block_serialization_max_endo_count() {
        let keypair =
            KeyPair::from_str("S1bXjyPwrssNmG4oUG5SEqaUhQkVArQi7rzQDWpCprTSmEgZDGG").unwrap();
        let endorsed = BlockId(
            Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf").unwrap(),
        );
        let fillers = (1..THREAD_COUNT).map(|i| BlockId(Hash::compute_from(&[i])));
        let parents = std::iter::once(endorsed).chain(fillers).collect();

        let endorsements = (0..ENDORSEMENT_COUNT)
            .map(|i| {
                Endorsement::new_verifiable(
                    Endorsement {
                        slot: Slot::new(1, 0),
                        index: i,
                        endorsed_block: BlockId(
                            Hash::from_bs58_check(
                                "bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf",
                            )
                            .unwrap(),
                        ),
                    },
                    EndorsementSerializer::new(),
                    &keypair,
                )
                .unwrap()
            })
            .collect();
        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 0),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements,
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let (_, res): (&[u8], SecureShareBlock) =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block)
                .unwrap();

        res.content
            .header
            .assert_invariants(THREAD_COUNT, ENDORSEMENT_COUNT)
            .unwrap();
    }
    #[test]
    #[serial]
    fn test_invalid_block_serialization_obo_low_parent_count() {
        let keypair = KeyPair::generate();
        // Non genesis block must have THREAD_COUNT parents
        let parents = (1..THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i])))
            .collect();

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
    }
    #[test]
    #[serial]
    fn test_invalid_block_serialization_obo_high_endo_count() {
        let keypair = KeyPair::generate();
        // Non genesis block must have THREAD_COUNT parents
        let parents = (0..THREAD_COUNT)
            .map(|i| BlockId(Hash::compute_from(&[i])))
            .collect();

        let endorsements = (0..=ENDORSEMENT_COUNT)
            .map(|i| {
                Endorsement::new_verifiable(
                    Endorsement {
                        slot: Slot::new(0, 1),
                        index: i,
                        endorsed_block: BlockId(Hash::compute_from(&[i as u8])),
                    },
                    EndorsementSerializer::new(),
                    &keypair,
                )
                .unwrap()
            })
            .collect();
        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements,
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: see issue #3400
        assert!(res.is_err());
    }
    #[test]
    #[serial]
    fn test_invalid_endorsement_idx() {
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

        let endo1 = Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 0),
                index: ENDORSEMENT_COUNT,
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
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 0),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![endo1],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);
        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
    }
    #[test]
    #[serial]
    fn test_invalid_dupe_endo_idx() {
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

        let endo1 = Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 0),
                index: 0,
                endorsed_block: BlockId(
                    Hash::from_bs58_check("bq1NsaCBAfseMKSjNBYLhpK7M5eeef2m277MYS2P2k424GaDf")
                        .unwrap(),
                ),
            },
            EndorsementSerializer::new(),
            &keypair,
        )
        .unwrap();
        let endo2 = Endorsement::new_verifiable(
            Endorsement {
                slot: Slot::new(1, 0),
                index: 0,
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
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                slot: Slot::new(1, 0),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![endo1, endo2],
                denunciations: vec![],
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
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let args = BlockDeserializerArgs {
            thread_count: THREAD_COUNT,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            endorsement_count: ENDORSEMENT_COUNT,
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            last_start_period: Some(0),
        };
        let res: Result<(&[u8], SecureShareBlock), _> =
            SecureShareDeserializer::new(BlockDeserializer::new(args))
                .deserialize::<DeserializeError>(&ser_block);

        // TODO: Catch an failed deser being a fail, instead of a recoverable error
        // TODO: assert that the error variant/context/etc. matches the expected failure
        assert!(res.is_err());
    }
}
