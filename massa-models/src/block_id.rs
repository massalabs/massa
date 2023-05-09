use crate::error::ModelsError;
use crate::prehash::PreHashed;
use crate::secure_share::Id;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use nom::error::{context, ContextError, ParseError};
use nom::IResult;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use std::collections::Bound::Included;
use std::convert::TryInto;
use std::str::FromStr;

/// Size in bytes of a serialized block ID
const BLOCK_ID_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// block id
#[derive(
    Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, SerializeDisplay, DeserializeFromStr,
)]
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
    /// # use massa_models::block_id::BlockId;
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
    pub(crate) fn into_bytes(self) -> [u8; BLOCK_ID_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// block id from bytes
    pub(crate) fn from_bytes(data: &[u8; BLOCK_ID_SIZE_BYTES]) -> BlockId {
        BlockId(Hash::from_bytes(data))
    }

    /// first bit of the hashed block id
    pub(crate) fn get_first_bit(&self) -> bool {
        self.to_bytes()[0] >> 7 == 1
    }
}

/// Serializer for `BlockId`
#[derive(Default, Clone)]
pub(crate) struct BlockIdSerializer;

impl BlockIdSerializer {
    /// Creates a new serializer for `BlockId`
    pub(crate) fn new() -> Self {
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
pub(crate) struct BlockIdDeserializer {
    hash_deserializer: HashDeserializer,
}

impl BlockIdDeserializer {
    /// Creates a new deserializer for `BlockId`
    pub(crate) fn new() -> Self {
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
