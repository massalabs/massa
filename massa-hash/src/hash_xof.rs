use std::ops::{BitXor, BitXorAssign};

use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{
    error::{context, ContextError, ParseError},
    IResult,
};

/// Extended Hash
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HashXof<const SIZE: usize>(pub [u8; SIZE]);

impl<const SIZE: usize> HashXof<SIZE> {
    /// From bytes
    pub fn from_bytes(bytes: &[u8; SIZE]) -> Self {
        HashXof(*bytes)
    }

    /// Transform into bytes
    pub fn to_bytes(&self) -> &[u8; SIZE] {
        &self.0
    }

    /// Compute from raw data
    pub fn compute_from(data: &[u8]) -> HashXof<SIZE> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(data);
        let mut hash = [0u8; SIZE];
        let mut output_reader = hasher.finalize_xof();
        output_reader.fill(&mut hash);
        HashXof(hash)
    }

    /// Compute from key and value
    pub fn compute_from_kv(key: &[u8], value: &[u8]) -> HashXof<SIZE> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&(key.len() as u64).to_be_bytes());
        hasher.update(key);
        hasher.update(value);
        let mut hash = [0u8; SIZE];
        let mut output_reader = hasher.finalize_xof();
        output_reader.fill(&mut hash);
        HashXof(hash)
    }

    /// Serialize a Hash using `bs58` encoding with checksum.
    /// Motivations for using base58 encoding:
    ///
    /// base58_check is like base64 but-
    /// * fully standardized (no = vs /)
    /// * no weird characters (eg. +) only alphanumeric
    /// * ambiguous letters combined (eg. O vs 0, or l vs 1)
    /// * contains a checksum at the end to detect typing errors
    ///    
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.0).with_check().into_string()
    }
}

// To use this xor operator you must ensure that you have all the criteria listed here :
// https://github.com/massalabs/massa/discussions/3852#discussioncomment-6188158
impl<const SIZE: usize> BitXorAssign for HashXof<SIZE> {
    fn bitxor_assign(&mut self, rhs: Self) {
        *self = *self ^ rhs;
    }
}

// To use this xor operator you must ensure that you have all the criteria listed here :
// https://github.com/massalabs/massa/discussions/3852#discussioncomment-6188158
impl<const SIZE: usize> BitXor for HashXof<SIZE> {
    type Output = Self;

    fn bitxor(self, other: Self) -> Self {
        let xored = self
            .0
            .iter()
            .zip(other.0.iter())
            .map(|(a, b)| a ^ b)
            .collect::<Vec<u8>>()
            .try_into()
            .unwrap();
        HashXof(xored)
    }
}

impl<const SIZE: usize> std::fmt::Display for HashXof<SIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_bs58_check())
    }
}

impl<const SIZE: usize> std::fmt::Debug for HashXof<SIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_bs58_check())
    }
}

/// Serializer for `HashXof`
#[derive(Default, Clone)]
pub struct HashXofSerializer;

impl HashXofSerializer {
    /// Creates a serializer for `HashXof`
    pub const fn new() -> Self {
        Self
    }
}

impl<const SIZE: usize> Serializer<HashXof<SIZE>> for HashXofSerializer {
    fn serialize(&self, value: &HashXof<SIZE>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `HashXof`
#[derive(Default, Clone)]
pub struct HashXofDeserializer;

impl HashXofDeserializer {
    /// Creates a deserializer for `HashXof`
    pub const fn new() -> Self {
        Self
    }
}

impl<const SIZE: usize> Deserializer<HashXof<SIZE>> for HashXofDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_hash::{HashXof, HASH_XOF_SIZE_BYTES, HashXofDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let hash_deserializer = HashXofDeserializer::new();
    /// let hash: HashXof<HASH_XOF_SIZE_BYTES> = HashXof::compute_from(&"hello world".as_bytes());
    /// let (rest, deserialized) = hash_deserializer.deserialize::<DeserializeError>(hash.to_bytes()).unwrap();
    /// assert_eq!(deserialized, hash);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], HashXof<SIZE>, E> {
        context("Failed hashxof deserialization", |input: &'a [u8]| {
            if buffer.len() < SIZE {
                return Err(nom::Err::Error(ParseError::from_error_kind(
                    input,
                    nom::error::ErrorKind::LengthValue,
                )));
            }
            Ok((
                &buffer[SIZE..],
                HashXof::from_bytes(&buffer[..SIZE].try_into().map_err(|_| {
                    nom::Err::Error(ParseError::from_error_kind(
                        input,
                        nom::error::ErrorKind::Fail,
                    ))
                })?),
            ))
        })(buffer)
    }
}
