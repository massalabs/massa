use bitcoin_hashes;

pub const HASH_SIZE_BYTES: usize = 32;

#[derive(Debug)]
pub enum HashError {
    ParseError,
}

impl std::error::Error for HashError {}

impl std::fmt::Display for HashError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            HashError::ParseError => write!(f, "parse error"),
        }
    }
}

#[derive(Eq, PartialEq, Copy, Clone, Hash)]
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
    /// let hash = Hash::hash(&data);
    /// ```
    pub fn hash(data: &[u8]) -> Self {
        use bitcoin_hashes::Hash;
        Hash(bitcoin_hashes::sha256::Hash::hash(data))
    }

    /// Serialize a Hash using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let serialized: String = hash.to_bs58_check();
    /// ```
    pub fn to_bs58_check(&self) -> String {
        bs58::encode(self.to_bytes()).with_check().into_string()
    }

    /// Serialize a Hash as bytes.
    ///
    /// # Example
    ///  ```
    /// let serialized: String = hash.to_bytes();
    /// ```
    pub fn to_bytes(&self) -> [u8; HASH_SIZE_BYTES] {
        use bitcoin_hashes::Hash;
        *self.0.as_inner()
    }

    /// Convert into bytes.
    ///
    /// # Example
    ///  ```
    /// let serialized: String = hash.into_bytes();
    /// ```
    pub fn into_bytes(self) -> [u8; HASH_SIZE_BYTES] {
        use bitcoin_hashes::Hash;
        self.0.into_inner()
    }

    /// Deserialize using bs58 encoding with checksum.
    ///
    /// # Example
    ///  ```
    /// let deserialized: Hash = Hash::from_bs58_check(data);
    /// ```
    pub fn from_bs58_check(data: &str) -> Result<Hash, HashError> {
        match bs58::decode(data).with_check(None).into_vec() {
            Ok(s) => Ok(Hash::from_bytes(&s)?),
            _ => Err(HashError::ParseError),
        }
    }

    /// Deserialize a Hash as bytes.
    ///
    /// # Example
    ///  ```
    /// let deserialized: Hash = Hash::from_bytes(data);
    /// ```
    pub fn from_bytes(data: &[u8]) -> Result<Hash, HashError> {
        use bitcoin_hashes::Hash;
        use std::convert::TryInto;
        let res_inner: Result<<bitcoin_hashes::sha256::Hash as Hash>::Inner, _> = data.try_into();
        match res_inner {
            Ok(inner) => Ok(Hash(bitcoin_hashes::sha256::Hash::from_inner(inner))),
            Err(_) => Err(HashError::ParseError),
        }
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
    /// let serialized: String = serde_json::to_string(&hash).unwrap();
    /// ```
    ///
    /// Not human readable serialization :
    /// ```
    /// let mut s = flexbuffers::FlexbufferSerializer::new();
    /// hash.serialize(&mut s).unwrap();
    /// let serialized = s.take_buffer();
    /// ```
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
    /// let deserialized: Hash = serde_json::from_str(&serialized).unwrap();
    /// ```
    ///
    /// Not human readable deserialization :
    /// ```
    /// let r = flexbuffers::Reader::get_root(serialized).unwrap();
    /// let deserialized: PrivateKey = PrivateKey::deserialize(r).unwrap();
    /// ```
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
                        Hash::from_bs58_check(&v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Hash::from_bs58_check(&v).map_err(E::custom)
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
                    Hash::from_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    fn example() -> Hash {
        Hash::hash(&"hello world".as_bytes())
    }

    #[test]
    fn test_serde_json() {
        let hash = example();
        let serialized = serde_json::to_string(&hash).unwrap();
        let deserialized = serde_json::from_str(&serialized).unwrap();
        assert_eq!(hash, deserialized)
    }

    #[test]
    fn test_flexbuffers() {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        let hash = example();
        hash.serialize(&mut s).unwrap();
        let r = flexbuffers::Reader::get_root(s.view()).unwrap();
        let deserialized = Hash::deserialize(r).unwrap();
        assert_eq!(deserialized, hash)
    }

    #[test]
    fn test_hash() {
        let data = "abc".as_bytes();
        let hash = Hash::hash(&data);
        let hash_ref: [u8; HASH_SIZE_BYTES] = [
            186, 120, 22, 191, 143, 1, 207, 234, 65, 65, 64, 222, 93, 174, 34, 35, 176, 3, 97, 163,
            150, 23, 122, 156, 180, 16, 255, 97, 242, 0, 21, 173,
        ];
        assert_eq!(hash.to_bytes(), hash_ref);
    }
}
