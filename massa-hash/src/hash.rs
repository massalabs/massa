// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::MassaHashError;
use crate::settings::HASH_SIZE_BYTES;
use bitcoin_hashes;
use std::{convert::TryInto, str::FromStr};

#[derive(Eq, PartialEq, Ord, PartialOrd, Copy, Clone, Hash)]
pub struct Hash(bitcoin_hashes::sha256::Hash);

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_bs58_check())
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.to_bs58_check())
    }
}

impl Hash {
    /// Compute a hash from data.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// ```
    pub fn compute_from(data: &[u8]) -> Self {
        use bitcoin_hashes::Hash;
        Hash(bitcoin_hashes::sha256::Hash::hash(data))
    }

    /// Serialize a Hash using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::hash::Hash;
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
    /// # use massa_hash::hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized = hash.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> [u8; HASH_SIZE_BYTES] {
        use bitcoin_hashes::Hash;
        *self.0.as_inner()
    }

    /// Convert into bytes.
    ///
    /// # Example
    ///  ```
    /// # use massa_hash::hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized = hash.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; HASH_SIZE_BYTES] {
        use bitcoin_hashes::Hash;
        self.0.into_inner()
    }

    /// Deserialize using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_hash::hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized: String = hash.to_bs58_check();
    /// let deserialized: Hash = Hash::from_bs58_check(&serialized).unwrap();
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Hash, MassaHashError> {
        let decoded_bs58_check = bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|err| MassaHashError::ParsingError(format!("{}", err)))?;
        Hash::from_bytes(
            &decoded_bs58_check
                .as_slice()
                .try_into()
                .map_err(|err| MassaHashError::ParsingError(format!("{}", err)))?,
        )
    }

    /// Deserialize a Hash as bytes.
    ///
    /// # Example
    ///  ```
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_hash::hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized = hash.into_bytes();
    /// let deserialized: Hash = Hash::from_bytes(&serialized).unwrap();
    /// ```
    pub fn from_bytes(data: &[u8; HASH_SIZE_BYTES]) -> Result<Hash, MassaHashError> {
        use bitcoin_hashes::Hash;
        Ok(Hash(
            bitcoin_hashes::sha256::Hash::from_slice(&data[..])
                .map_err(|err| MassaHashError::ParsingError(format!("{}", err)))?,
        ))
    }
}

impl ::serde::Serialize for Hash {
    /// ::serde::Serialize trait for Hash
    /// if the serializer is human readable,
    /// serialization is done using serialize_bs58_check
    /// else, it uses serialize_binary
    ///
    /// # Example
    ///
    /// Human readable serialization :
    /// ```
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_hash::hash::Hash;
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized: String = serde_json::to_string(&hash).unwrap();
    /// ```
    ///
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_bs58_check())
        } else {
            s.serialize_bytes(&self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Hash {
    /// ::serde::Deserialize trait for Hash
    /// if the deserializer is human readable,
    /// deserialization is done using deserialize_bs58_check
    /// else, it uses deserialize_binary
    ///
    /// # Example
    ///
    /// Human readable deserialization :
    /// ```
    /// # use massa_hash::hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// let hash = Hash::compute_from(&"hello world".as_bytes());
    /// let serialized: String = serde_json::to_string(&hash).unwrap();
    /// let deserialized: Hash = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Hash, D::Error> {
        if d.is_human_readable() {
            struct Base58CheckVisitor;

            impl<'de> ::serde::de::Visitor<'de> for Base58CheckVisitor {
                type Value = Hash;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("an ASCII base58check string")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Hash::from_bs58_check(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bs58_check(v).map_err(E::custom)
                }
            }
            d.deserialize_str(Base58CheckVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Hash;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bytes(v.try_into().map_err(E::custom)?).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl FromStr for Hash {
    type Err = MassaHashError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Hash::from_bs58_check(s)
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    fn example() -> Hash {
        Hash::compute_from("hello world".as_bytes())
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
        let hash = Hash::compute_from(data);
        let hash_ref: [u8; HASH_SIZE_BYTES] = [
            186, 120, 22, 191, 143, 1, 207, 234, 65, 65, 64, 222, 93, 174, 34, 35, 176, 3, 97, 163,
            150, 23, 122, 156, 180, 16, 255, 97, 242, 0, 21, 173,
        ];
        assert_eq!(hash.to_bytes(), hash_ref);
    }
}
