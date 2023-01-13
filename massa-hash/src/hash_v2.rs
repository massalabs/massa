// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::MassaHashError;
use crate::settings::HASHV2_SIZE_BYTES;
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{
    error::{context, ContextError, ParseError},
    IResult,
};
use std::{
    convert::TryInto,
    ops::{BitXor, BitXorAssign},
    str::FromStr,
};
use std::hash::Hash;

use sha2::{Sha512, Digest};

/// Hash wrapper, the underlying hash type is `Sha512`
#[derive(Eq, PartialEq, PartialOrd, Ord, Hash, Copy, Clone, Debug)]
pub struct HashV2([u8; 64]);

impl std::fmt::Display for HashV2 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_bs58_check())
    }
}

impl BitXorAssign for HashV2 {
    fn bitxor_assign(&mut self, rhs: Self) {
        *self = *self ^ rhs;
    }
}

impl BitXor for HashV2 {
    type Output = Self;

    fn bitxor(self, other: Self) -> Self {
        let xored_bytes: Vec<u8> = self
            .to_bytes()
            .iter()
            .zip(other.to_bytes())
            .map(|(x, y)| x ^ y)
            .collect();
        // unwrap won't fail because of the intial byte arrays size
        let input_bytes: [u8; HASHV2_SIZE_BYTES] = xored_bytes.try_into().unwrap();
        HashV2::from_bytes(&input_bytes)
    }
}

impl HashV2 {
    /// Compute a hash from data.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// ```
    pub fn compute_from(data: &[u8]) -> Self {

        // same for Sha512
        let mut hasher = Sha512::new();
        hasher.update(data);
        let result = hasher.finalize();

        HashV2(result.into())
    }

    /// Serialize a Hash using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized: String = hash.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a Hash as bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized = hash.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> &[u8; HASHV2_SIZE_BYTES] {
        &self.0
    }

    /// Convert into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized = hash.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; HASHV2_SIZE_BYTES] {
        self.0
    }

    /// Deserialize using `bs58` encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized: String = hash.to_bs58_check();
    /// let deserialized: Hash = Hash::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<HashV2, MassaHashError> {
        let decoded_bs58_check = bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| MassaHashError::ParsingError(format!("{}", err)))?;
        Ok(HashV2::from_bytes(
            &decoded_bs58_check
                .as_slice()
                .try_into()
                .map_err(|err| MassaHashError::ParsingError(format!("{}", err)))?,
        ))
    }

    /// Deserialize a Hash as bytes.
    ///
    /// # Example
    ///  ```
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized = hash.into_bytes();
    /// let deserialized: Hash = Hash::from_bytes(&serialized);
    /// ```
    pub fn from_bytes(data: &[u8; HASHV2_SIZE_BYTES]) -> HashV2 {
        HashV2(*data)
    }
}

/// Serializer for `Hash`
#[derive(Default)]
pub struct HashV2Serializer;

impl HashV2Serializer {
    /// Creates a serializer for `Hash`
    pub const fn new() -> Self {
        Self
    }
}

impl Serializer<HashV2> for HashV2Serializer {
    fn serialize(&self, value: &HashV2, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `Hash`
#[derive(Default, Clone)]
pub struct HashV2Deserializer;

impl HashV2Deserializer {
    /// Creates a deserializer for `Hash`
    pub const fn new() -> Self {
        Self
    }
}

impl Deserializer<HashV2> for HashV2Deserializer {
    /// ## Example
    /// ```rust
    /// use massa_hash::{Hash, HashDeserializer};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    ///
    /// let hash_deserializer = HashDeserializer::new();
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let (rest, deserialized) = hash_deserializer.deserialize::<DeserializeError>(hash.to_bytes()).unwrap();
    /// assert_eq!(deserialized, hash);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], HashV2, E> {
        context("Failed hash deserialization", |input: &'a [u8]| {
            if buffer.len() < HASHV2_SIZE_BYTES {
                return Err(nom::Err::Error(ParseError::from_error_kind(
                    input,
                    nom::error::ErrorKind::LengthValue,
                )));
            }
            Ok((
                &buffer[HASHV2_SIZE_BYTES..],
                HashV2::from_bytes(&buffer[..HASHV2_SIZE_BYTES].try_into().map_err(|_| {
                    nom::Err::Error(ParseError::from_error_kind(
                        input,
                        nom::error::ErrorKind::Fail,
                    ))
                })?),
            ))
        })(buffer)
    }
}

impl ::serde::Serialize for HashV2 {
    /// `::serde::Serialize` trait for Hash
    /// if the serializer is human readable,
    /// serialization is done using `serialize_bs58_check`
    /// else, it uses `serialize_binary`
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_hash::HashV2;
    /// let hash = HashV2::compute_from(&"hello world".as_bytes());
    /// let serialized: String = serde_json::to_string(&hash).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(&self.0)
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for HashV2 {
    /// `::serde::Deserialize` trait for Hash
    /// if the deserializer is human readable,
    /// deserialization is done using `deserialize_bs58_check`
    /// else, it uses `deserialize_binary`
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized: String = serde_json::to_string(&hash).unwrap();
    /// let deserialized: Hash = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<HashV2, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = HashV2;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        HashV2::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    HashV2::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = HashV2;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Ok(HashV2::from_bytes(v.try_into().map_err(E::custom)?))
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl FromStr for HashV2 {
    type Err = MassaHashError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        HashV2::from_bs58_check(s)
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    fn example() -> HashV2 {
        HashV2::compute_from("hello world".as_bytes())
    }

    #[test]
    #[serial]
    fn test_serde_json() {
        let hash = example();
        let serialized = serde_json::to_string(&hash).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(hash, deserialized)
    }

    #[test]
    #[serial]
    fn test_hash() {
        let data = "abc".as_bytes();
        let hash = HashV2::compute_from(data);
        let hash_ref: [u8; HASHV2_SIZE_BYTES] = [221, 175, 53, 161, 147, 97, 122, 186, 204, 65, 115, 73, 174, 32,
        65, 49, 18, 230, 250, 78, 137, 169, 126, 162, 10, 158, 238, 230, 75, 85, 211, 154, 33, 146, 153, 42, 39,
        79, 193, 168, 54, 186, 60, 35, 163, 254, 235, 189, 69, 77, 68, 35, 100, 60, 232, 14, 42, 154, 201, 79,
        165, 76, 164, 159
        ];
        assert_eq!(hash.into_bytes(), hash_ref);
    }
}
