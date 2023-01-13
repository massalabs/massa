//! Copyright (c) 2022 MASSA LABS <info@massa.net>

// use crate::endorsement::{EndorsementId, EndorsementSerializer, EndorsementSerializerLW};
// use crate::prehash::PreHashed;
use crate::secure_share::{
    Id,
    SecureShare,
    SecureShareContent,
    SecureShareDeserializer,
    // SecureShareSerializer,
};
use crate::{
    // endorsement::{Endorsement, EndorsementDeserializerLW, SecureShareEndorsement},
    error::ModelsError,
    operation::{
        OperationId,
        // OperationIdsDeserializer,
        // OperationIdsSerializer,
        SecureShareOperation,
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
// use nom::sequence::{
//     preceded,
//     tuple};
use nom::Parser;
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use serde::{Deserialize, Serialize};
// use serde_with::{DeserializeFromStr, SerializeDisplay};
// use std::convert::TryInto;
use std::fmt::Formatter;
// use std::ops::Bound::{Excluded, Included};
// use std::str::FromStr;
use crate::block_header::{BlockHeader, BlockHeaderDeserializer, SecuredHeader};
use crate::block_id::BlockId;
use crate::block_v0::{BlockV0, BlockV0Deserializer, BlockV0Serializer, FilledBlockV0};
use crate::block_v1::{BlockV1, BlockV1Deserializer, BlockV1Serializer, FilledBlockV1};

/// block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Block {
    /// Block v1 (initial)
    V0(BlockV0),
    /// Block v2 (with denunciations)
    V1(BlockV1),
}

/// filled block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilledBlock {
    /// Block v1 (initial)
    V0(FilledBlockV0),
    /// Block v2 (with denunciations)
    V1(FilledBlockV1),
}

impl Block {
    /// Get header
    pub fn header(&self) -> &SecuredHeader {
        match self {
            Block::V0(block_v0) => &block_v0.header,
            Block::V1(block_v1) => &block_v1.header,
        }
    }
    /// Get header mut
    pub fn header_mut(&mut self) -> &mut SecuredHeader {
        match self {
            Block::V0(block_v0) => &mut block_v0.header,
            Block::V1(block_v1) => &mut block_v1.header,
        }
    }
    /// Get operations
    pub fn operations(&self) -> &Vec<OperationId> {
        match self {
            Block::V0(block_v0) => &block_v0.operations,
            Block::V1(block_v1) => &block_v1.operations,
        }
    }
    /// Get operations mut
    pub fn operations_mut(&mut self) -> &mut Vec<OperationId> {
        match self {
            Block::V0(block_v0) => &mut block_v0.operations,
            Block::V1(block_v1) => &mut block_v1.operations,
        }
    }
}

/// Block with assosciated meta-data and interfaces allowing trust of data in untrusted network
pub type SecureShareBlock = SecureShare<Block, BlockId>;

impl SecureShareContent for Block {
    fn new_verifiable<SC: Serializer<Self>, U: Id>(
        content: Self,
        content_serializer: SC,
        _keypair: &KeyPair,
    ) -> Result<SecureShare<Self, U>, ModelsError> {
        let mut content_serialized = Vec::new();
        content_serializer.serialize(&content, &mut content_serialized)?;
        Ok(SecureShare {
            signature: content.header().signature,
            content_creator_pub_key: content.header().content_creator_pub_key,
            content_creator_address: content.header().content_creator_address,
            id: U::new(*content.header().id.get_hash()),
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
                signature: content.header().signature,
                content_creator_pub_key: content.header().content_creator_pub_key,
                content_creator_address: content.header().content_creator_address,
                id: U::new(*content.header().id.get_hash()),
                content,
                serialized_data: buffer[..buffer.len() - rest.len()].to_vec(),
            },
        ))
    }
}
/// Serializer for `Block`
pub struct BlockSerializer {
    // header_serializer: SecureShareSerializer,
    // op_ids_serializer: OperationIdsSerializer,
}

impl BlockSerializer {
    /// Creates a new `BlockSerializer`
    pub fn new() -> Self {
        BlockSerializer {
            // header_serializer: SecureShareSerializer::new(),
            // op_ids_serializer: OperationIdsSerializer::new(),
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
    /// use massa_models::block_id::BlockId;
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_hash::Hash;
    /// use massa_models::block_v0::BlockV0;
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
    ///         block_version_current: 0, block_version_next: 0, slot: Slot::new(1, 1),
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
    /// let orig_block = Block::V0(BlockV0 {
    ///     header: orig_header,
    ///     operations: Vec::new(),
    /// });
    ///
    /// let mut buffer = Vec::new();
    /// BlockSerializer::new().serialize(&orig_block, &mut buffer).unwrap();
    /// ```

    fn serialize(&self, value: &Block, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            Block::V0(block_v0) => {
                let ser = BlockV0Serializer::new();
                ser.serialize(block_v0, buffer)?;
            }
            Block::V1(block_v1) => {
                let ser = BlockV1Serializer::new();
                ser.serialize(block_v1, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `Block`
pub struct BlockDeserializer {
    header_deserializer: SecureShareDeserializer<BlockHeader, BlockHeaderDeserializer>,
    // op_ids_deserializer: OperationIdsDeserializer,
    // TODO: init Deser obj in new()?
    thread_count: u8,
    max_operations_per_block: u32,
    endorsement_count: u32, // TODO: max_denunciations_per_block
}

impl BlockDeserializer {
    /// Creates a new `BlockDeserializer`
    pub fn new(thread_count: u8, max_operations_per_block: u32, endorsement_count: u32) -> Self {
        BlockDeserializer {
            header_deserializer: SecureShareDeserializer::new(BlockHeaderDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            // op_ids_deserializer: OperationIdsDeserializer::new(max_operations_per_block),
            thread_count,
            max_operations_per_block,
            endorsement_count,
        }
    }
}

impl Deserializer<Block> for BlockDeserializer {
    /// ## Example:
    /// ```rust
    /// use massa_models::{block::{Block, BlockSerializer, BlockDeserializer}, config::THREAD_COUNT, slot::Slot, endorsement::{Endorsement, EndorsementSerializer}, secure_share::SecureShareContent, prehash::PreHashSet};
    /// use massa_models::block_id::BlockId;
    /// use massa_models::block_header::{BlockHeader, BlockHeaderSerializer};
    /// use massa_hash::Hash;
    /// use massa_models::block_v0::BlockV0;
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
    ///         block_version_current: 0,
    ///         block_version_next: 0,
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
    /// let orig_block = Block::V0(BlockV0 {
    ///     header: orig_header,
    ///     operations: Vec::new(),
    /// });
    ///
    /// let mut buffer = Vec::new();
    /// BlockSerializer::new().serialize(&orig_block, &mut buffer).unwrap();
    /// let (rest, res_block) = BlockDeserializer::new(THREAD_COUNT, 100, 9).deserialize::<DeserializeError>(&mut buffer).unwrap();
    ///
    /// assert!(rest.is_empty());
    /// // check equality
    /// assert_eq!(orig_block.header().id, res_block.header().id);
    /// assert_eq!(
    ///     orig_block.header().content.slot,
    ///     res_block.header().content.slot
    /// );
    /// assert_eq!(
    ///     orig_block.header().serialized_data,
    ///     res_block.header().serialized_data
    /// );
    /// assert_eq!(
    ///     orig_block.header().signature,
    ///     res_block.header().signature
    /// );
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Block, E> {
        let (_rem, block_header): (&[u8], SecuredHeader) = context(
            "Failed Block deserialization",
            context("Failed Block header deser", |input| {
                self.header_deserializer.deserialize(input)
            }),
        )
        .parse(buffer)?;

        // FIXME: a bit suboptimal as we parse the whole buffer
        //        Parse only the version in block_header to avoid parsing the whole header?
        match block_header.content.block_version_current {
            0 => context(
                "Failed block v0 deserialization",
                context("Failed block v0 deser", |_input| {
                    let der = BlockV0Deserializer::new(
                        self.thread_count,
                        self.max_operations_per_block,
                        self.endorsement_count,
                    );
                    der.deserialize(buffer)
                }),
            )
            .map(|block_v0| Block::V0(block_v0))
            .parse(buffer),
            _ => context(
                "Failed block v1 deserialization",
                context("Failed block v1 deser", |_input| {
                    let der = BlockV1Deserializer::new(
                        self.thread_count,
                        self.max_operations_per_block,
                        self.endorsement_count,
                    );
                    der.deserialize(buffer)
                }),
            )
            .map(|block_v2| Block::V1(block_v2))
            .parse(buffer),
        }
    }
}

impl SecureShareBlock {
    /// size in bytes of the whole block
    pub fn bytes_count(&self) -> u64 {
        self.serialized_data.len() as u64
    }

    /// true if given operation is included in the block
    pub fn contains_operation(&self, op: SecureShareOperation) -> bool {
        self.content.operations().contains(&op.id)
    }

    /// returns the fitness of the block
    pub fn get_fitness(&self) -> u64 {
        self.content.header().get_fitness()
    }
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.header())?;
        writeln!(
            f,
            "Operations: {}",
            self.operations()
                .iter()
                .map(|op| format!("{}", op))
                .collect::<Vec<String>>()
                .join(" ")
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::denunciation::Denunciation;
    use crate::secure_share::SecureShareSerializer;
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

        let endo = Endorsement::new_verifiable(
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
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                block_version_current: 0,
                block_version_next: 0,
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
        let orig_block = Block::V0(BlockV0 {
            header: orig_header.clone(),
            operations: Default::default(),
        });

        // serialize block
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let (rest, res_block): (&[u8], SecureShareBlock) = SecureShareDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block)
        .unwrap();

        assert!(rest.is_empty());
        // check equality
        assert_eq!(orig_block.header().id, res_block.content.header().id);
        assert_eq!(orig_block.header().id, res_block.id);
        assert_eq!(
            orig_block.header().content.slot,
            res_block.content.header().content.slot
        );
        assert_eq!(
            orig_block.header().serialized_data,
            res_block.content.header().serialized_data
        );
        assert_eq!(
            orig_block.header().signature,
            res_block.content.header().signature
        );

        assert_eq!(
            orig_block.header().content.endorsements,
            res_block.content.header().content.endorsements
        );

        assert_eq!(orig_block.header().signature, res_block.signature);
        orig_header.verify_signature().unwrap();
        for ed in orig_block.header().content.endorsements.iter() {
            ed.verify_signature().unwrap();
        }
        res_block.content.header().verify_signature().unwrap();
        for ed in res_block.content.header().content.endorsements.iter() {
            ed.verify_signature().unwrap();
        }
    }

    #[test]
    #[serial]
    fn test_block_v1_serialization() {
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

        let endo = Endorsement::new_verifiable(
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
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                block_version_current: 1,
                block_version_next: 1,
                slot: Slot::new(1, 0),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![endo],
            },
            BlockHeaderSerializer::new(),
            &keypair,
        )
        .unwrap();

        // Denunciation

        let de1_keypair =
            KeyPair::from_str("S1bXjyPwrssNmG4oUG5SEqaUhQkVArQi7rzQDWpCprTSmEgZDGG").unwrap();
        let de2_keypair =
            KeyPair::from_str("S1bXjyPwrssNmG4oUG5SEqaUhQkVArQi7rzQDWpCprTSmEgZDGG").unwrap();
        let de1 = Denunciation {
            slot: Slot::new(0, 11),
            pub_key: de1_keypair.get_public_key(),
            proof: true,
        };
        let de2 = Denunciation {
            slot: Slot::new(0, 14),
            pub_key: de2_keypair.get_public_key(),
            proof: true,
        };

        // create block
        let orig_block = Block::V1(BlockV1 {
            header: orig_header.clone(),
            denunciations: vec![de1, de2],
            operations: Default::default(),
        });

        // serialize block
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let (rest, res_block): (&[u8], SecureShareBlock) = SecureShareDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block)
        .unwrap();

        assert!(rest.is_empty());
        // check equality
        assert_eq!(orig_block.header().id, res_block.content.header().id);
        assert_eq!(orig_block.header().id, res_block.id);
        assert_eq!(
            orig_block.header().content.slot,
            res_block.content.header().content.slot
        );
        assert_eq!(
            orig_block.header().serialized_data,
            res_block.content.header().serialized_data
        );
        assert_eq!(
            orig_block.header().signature,
            res_block.content.header().signature
        );

        assert_eq!(
            orig_block.header().content.endorsements,
            res_block.content.header().content.endorsements
        );

        assert_eq!(orig_block.header().signature, res_block.signature);
        orig_header.verify_signature().unwrap();
        for ed in orig_block.header().content.endorsements.iter() {
            ed.verify_signature().unwrap();
        }
        res_block.content.header().verify_signature().unwrap();
        for ed in res_block.content.header().content.endorsements.iter() {
            ed.verify_signature().unwrap();
        }
    }

    #[test]
    #[serial]
    fn test_genesis_block_serialization() {
        let keypair = KeyPair::generate();
        let parents: Vec<BlockId> = vec![];

        // create block header
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                block_version_current: 0,
                block_version_next: 0,
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
        let orig_block = Block::V0(BlockV0 {
            header: orig_header,
            operations: Default::default(),
        });

        // serialize block
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block.clone(), BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let (rest, res_block): (&[u8], SecureShareBlock) = SecureShareDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block)
        .unwrap();

        // check equality

        assert!(rest.is_empty());
        assert_eq!(orig_block.header().id, res_block.content.header().id);
        // assert_eq!(orig_block.header.id, res_block.id);
        assert_eq!(
            orig_block.header().content.slot,
            res_block.content.header().content.slot
        );
        assert_eq!(
            orig_block.header().serialized_data,
            res_block.content.header().serialized_data
        );
        assert_eq!(
            orig_block.header().signature,
            res_block.content.header().signature
        );

        assert_eq!(
            orig_block.header().content.endorsements,
            res_block.content.header().content.endorsements
        );

        assert_eq!(orig_block.header().signature, res_block.signature);
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
        let orig_header = BlockHeader::new_verifiable(
            BlockHeader {
                block_version_current: 0,
                block_version_next: 0,
                slot: Slot::new(1, 1),
                parents,
                operation_merkle_root: Hash::compute_from("mno".as_bytes()),
                endorsements: vec![Endorsement::new_verifiable(
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
        let orig_block = Block::V0(BlockV0 {
            header: orig_header,
            operations: Default::default(),
        });

        // serialize block
        let secured_block: SecureShareBlock =
            Block::new_verifiable(orig_block, BlockSerializer::new(), &keypair).unwrap();
        let mut ser_block = Vec::new();
        SecureShareSerializer::new()
            .serialize(&secured_block, &mut ser_block)
            .unwrap();

        // deserialize
        let res: Result<(&[u8], SecureShareBlock), _> = SecureShareDeserializer::new(
            BlockDeserializer::new(THREAD_COUNT, MAX_OPERATIONS_PER_BLOCK, ENDORSEMENT_COUNT),
        )
        .deserialize::<DeserializeError>(&ser_block);

        assert!(res.is_err());
    }
}
