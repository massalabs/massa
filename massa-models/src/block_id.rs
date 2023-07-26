use crate::error::ModelsError;
use crate::prehash::PreHashed;
use crate::secure_share::Id;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ErrorKind, ParseError},
    IResult,
};
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::collections::Bound::Included;
use std::str::FromStr;
use transition::Versioned;

/// block id
#[allow(missing_docs)]
#[transition::versioned(versions("0"))]
#[derive(
    Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, SerializeDisplay, DeserializeFromStr,
)]
pub struct BlockId(pub Hash);

impl PreHashed for BlockId {}

impl Id for BlockId {
    fn new(hash: Hash) -> Self {
        BlockId::BlockIdV0(BlockIdV0(hash))
    }

    fn get_hash(&self) -> &Hash {
        match self {
            BlockId::BlockIdV0(block_id) => block_id.get_hash(),
        }
    }
}

impl BlockId {
    /// first bit of the hashed block id
    pub fn get_first_bit(&self) -> bool {
        match self {
            BlockId::BlockIdV0(block_id) => block_id.get_first_bit(),
        }
    }

    /// version of the block id
    pub fn get_version(&self) -> u64 {
        match self {
            BlockId::BlockIdV0(block_id) => block_id.get_version(),
        }
    }

    /// Generate a version 0 block id from an hash used only for tests
    #[cfg(any(test, feature = "testing"))]
    pub fn generate_from_hash(hash: Hash) -> BlockId {
        BlockId::BlockIdV0(BlockIdV0(hash))
    }
}

#[transition::impl_version(versions("0"))]
impl BlockId {
    fn get_hash(&self) -> &Hash {
        &self.0
    }

    /// first bit of the hashed block id
    pub fn get_first_bit(&self) -> bool {
        self.0.to_bytes()[0] >> 7 == 1
    }

    /// version of the block id
    pub fn get_version(&self) -> u64 {
        Self::VERSION
    }
}

const BLOCKID_PREFIX: char = 'B';

impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockId::BlockIdV0(block_id) => write!(f, "{}", block_id),
        }
    }
}

#[transition::impl_version(versions("0"))]
impl std::fmt::Display for BlockId {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&Self::VERSION, &mut bytes)
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
    /// # use massa_models::block_id::BlockId;
    /// # use crate::massa_models::secure_share::Id;
    /// # let hash = Hash::compute_from(b"test");
    /// # let block_id = BlockId::new(hash);
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
                let block_id_deserializer = BlockIdDeserializer::new();
                let (rest, block_id) = block_id_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::OperationIdParseError)?;
                if rest.is_empty() {
                    Ok(block_id)
                } else {
                    Err(ModelsError::OperationIdParseError)
                }
            }
            _ => Err(ModelsError::BlockIdParseError),
        }
    }
}

#[transition::impl_version(versions("0"))]
impl FromStr for BlockId {
    type Err = ModelsError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == BLOCKID_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::BlockIdParseError)?;
                let block_id_deserializer = BlockIdDeserializer::new();
                let (rest, block_id) = block_id_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::OperationIdParseError)?;
                if rest.is_empty() {
                    Ok(block_id)
                } else {
                    Err(ModelsError::OperationIdParseError)
                }
            }
            _ => Err(ModelsError::BlockIdParseError),
        }
    }
}

/// Serializer for `BlockId`
#[derive(Default, Clone)]
pub struct BlockIdSerializer {
    version_serializer: U64VarIntSerializer,
}

impl BlockIdSerializer {
    /// Creates a new serializer for `BlockId`
    pub fn new() -> Self {
        Self {
            version_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<BlockId> for BlockIdSerializer {
    fn serialize(&self, value: &BlockId, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.version_serializer
            .serialize(&value.get_version(), buffer)?;
        match value {
            BlockId::BlockIdV0(block_id) => self.serialize(block_id, buffer),
        }
    }
}

#[transition::impl_version(versions("0"), structures("BlockId"))]
impl Serializer<BlockId> for BlockIdSerializer {
    fn serialize(&self, value: &BlockId, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.0.to_bytes());
        Ok(())
    }
}

/// Deserializer for `BlockId`
#[derive(Clone)]
pub struct BlockIdDeserializer {
    hash_deserializer: HashDeserializer,
    version_deserializer: U64VarIntDeserializer,
}

impl Default for BlockIdDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockIdDeserializer {
    /// Creates a new deserializer for `BlockId`
    pub fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
            version_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        }
    }
}

impl Deserializer<BlockId> for BlockIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockId, E> {
        // Verify that we at least have a version and something else
        if buffer.len() < 2 {
            return Err(nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof)));
        }
        let (rest, op_id_version) =
            self.version_deserializer
                .deserialize(buffer)
                .map_err(|_: nom::Err<E>| {
                    nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof))
                })?;
        match op_id_version {
            <BlockId!["0"]>::VERSION => {
                let (rest, op_id) = self.deserialize(rest)?;
                Ok((rest, BlockIdVariant!["0"](op_id)))
            }
            _ => Err(nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof))),
        }
    }
}

#[transition::impl_version(versions("0"), structures("BlockId"))]
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
