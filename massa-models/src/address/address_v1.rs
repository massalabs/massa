// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::prehash::PreHashed;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::PublicKey;
use nom::error::{context, ContextError, ParseError};
use nom::{IResult, Parser};
use std::ops::Bound::Included;
use std::str::FromStr;

/// Size of a serialized AddressV1, in bytes
pub const ADDRESSV1_SIZE_BYTES: usize = massa_hash::HASHV1_SIZE_BYTES;

/// Derived from a public key
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AddressV1(pub Hash);

const ADDRESSV1_PREFIX: char = 'A';
pub const ADDRESSV1_VERSION: u64 = 0;

impl std::fmt::Display for AddressV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&ADDRESSV1_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            ADDRESSV1_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for AddressV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl ::serde::Serialize for AddressV1 {
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_string())
        } else {
            s.serialize_bytes(self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for AddressV1 {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<AddressV1, D::Error> {
        if d.is_human_readable() {
            struct AddressV1Visitor;

            impl<'de> ::serde::de::Visitor<'de> for AddressV1Visitor {
                type Value = AddressV1;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("A + base58::encode(version + hash)")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        AddressV1::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    AddressV1::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(AddressV1Visitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = AddressV1;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Ok(AddressV1::from_bytes(v.try_into().map_err(E::custom)?))
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl FromStr for AddressV1 {
    type Err = ModelsError;
    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use std::str::FromStr;
    /// # use massa_models::AddressV1::AddressV1;
    /// # let keypair = KeyPair::generate();
    /// # let AddressV1 = AddressV1::from_public_key(&keypair.get_public_key());
    /// let ser = AddressV1.to_string();
    /// let res_addr = AddressV1::from_str(&ser).unwrap();
    /// assert_eq!(AddressV1, res_addr);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == ADDRESSV1_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::AddressParseError)?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::AddressParseError)?;
                Ok(AddressV1(Hash::from_bytes(
                    rest.try_into()
                        .map_err(|_| ModelsError::AddressParseError)?,
                )))
            }
            _ => Err(ModelsError::AddressParseError),
        }
    }
}

#[test]
fn test_address_v1_str_format() {
    /*  use massa_signature::KeyPair;

    let keypair = KeyPair::generate();
    let address = AddressV1::from_public_key(&keypair.get_public_key());
    let a = AddressV1.to_string();
    let b = AddressV1::from_str(&a).unwrap();
    assert!(address == b);*/
}

impl PreHashed for AddressV1 {}

impl AddressV1 {
    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.to_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }

    /// Computes AddressV1 associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        AddressV1(Hash::compute_from(public_key.to_bytes()))
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::AddressV1::AddressV1;
    /// # let keypair = KeyPair::generate();
    /// # let AddressV1 = AddressV1::from_public_key(&keypair.get_public_key());
    /// let bytes = AddressV1.into_bytes();
    /// let res_addr = AddressV1::from_bytes(&bytes);
    /// assert_eq!(AddressV1, res_addr);
    /// ```
    pub fn to_bytes(&self) -> &[u8; ADDRESSV1_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::AddressV1::AddressV1;
    /// # let keypair = KeyPair::generate();
    /// # let AddressV1 = AddressV1::from_public_key(&keypair.get_public_key());
    /// let bytes = AddressV1.into_bytes();
    /// let res_addr = AddressV1::from_bytes(&bytes);
    /// assert_eq!(AddressV1, res_addr);
    /// ```
    pub fn into_bytes(self) -> [u8; ADDRESSV1_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::AddressV1::AddressV1;
    /// # let keypair = KeyPair::generate();
    /// # let AddressV1 = AddressV1::from_public_key(&keypair.get_public_key());
    /// let bytes = AddressV1.to_bytes();
    /// let res_addr = AddressV1::from_bytes(&bytes);
    /// assert_eq!(AddressV1, res_addr);
    /// ```
    pub fn from_bytes(data: &[u8; ADDRESSV1_SIZE_BYTES]) -> AddressV1 {
        AddressV1(Hash::from_bytes(data))
    }
}

/// Serializer for `AddressV1`
#[derive(Default, Clone)]
pub struct AddressV1Serializer;

impl AddressV1Serializer {
    /// Serializes an `AddressV1` into a `Vec<u8>`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<AddressV1> for AddressV1Serializer {
    fn serialize(
        &self,
        value: &AddressV1,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `AddressV1`
#[derive(Default, Clone)]
pub struct AddressV1Deserializer {
    hash_deserializer: HashDeserializer,
}

impl AddressV1Deserializer {
    /// Creates a new deserializer for `AddressV1`
    pub const fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<AddressV1> for AddressV1Deserializer {
    /// ## Example
    /// ```rust
    /// use massa_models::AddressV1::{AddressV1, AddressV1Deserializer};
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let AddressV1 = AddressV1::from_str("A12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap();
    /// let bytes = AddressV1.into_bytes();
    /// let (rest, res_addr) = AddressV1Deserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// assert_eq!(AddressV1, res_addr);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AddressV1, E> {
        context("Failed AddressV1 deserialization", |input| {
            self.hash_deserializer.deserialize(input)
        })
        .map(AddressV1)
        .parse(buffer)
    }
}
