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
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;
use std::str::FromStr;

/// Size of a serialized address, in bytes
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// Derived from a public key
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Address(pub Hash);

const ADDRESS_PREFIX: char = 'A';
const ADDRESS_VERSION: u64 = 0;

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&ADDRESS_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(
            f,
            "{}{}",
            ADDRESS_PREFIX,
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl ::serde::Serialize for Address {
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_string())
        } else {
            s.serialize_bytes(self.to_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for Address {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<Address, D::Error> {
        if d.is_human_readable() {
            struct AddressVisitor;

            impl<'de> ::serde::de::Visitor<'de> for AddressVisitor {
                type Value = Address;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("A + base58::encode(version + hash)")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Address::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Address::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(AddressVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = Address;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Ok(Address::from_bytes(v.try_into().map_err(E::custom)?))
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl FromStr for Address {
    type Err = ModelsError;
    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use std::str::FromStr;
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let ser = address.to_string();
    /// let res_addr = Address::from_str(&ser).unwrap();
    /// assert_eq!(address, res_addr);
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        match chars.next() {
            Some(prefix) if prefix == ADDRESS_PREFIX => {
                let data = chars.collect::<String>();
                let decoded_bs58_check = bs58::decode(data)
                    .with_check(None)
                    .into_vec()
                    .map_err(|_| ModelsError::AddressParseError)?;
                let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
                let (rest, _version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::AddressParseError)?;
                Ok(Address(Hash::from_bytes(
                    rest.try_into()
                        .map_err(|_| ModelsError::AddressParseError)?,
                )))
            }
            _ => Err(ModelsError::AddressParseError),
        }
    }
}

#[test]
fn test_address_str_format() {
    use massa_signature::KeyPair;

    let keypair = KeyPair::generate();
    let address = Address::from_public_key(&keypair.get_public_key());
    let a = address.to_string();
    let b = Address::from_str(&a).unwrap();
    assert!(address == b);
}

impl PreHashed for Address {}

impl Address {
    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.to_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }

    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Address(Hash::compute_from(public_key.to_bytes()))
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = address.into_bytes();
    /// let res_addr = Address::from_bytes(&bytes);
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn to_bytes(&self) -> &[u8; ADDRESS_SIZE_BYTES] {
        self.0.to_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = address.into_bytes();
    /// let res_addr = Address::from_bytes(&bytes);
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn into_bytes(self) -> [u8; ADDRESS_SIZE_BYTES] {
        self.0.into_bytes()
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = address.to_bytes();
    /// let res_addr = Address::from_bytes(&bytes);
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn from_bytes(data: &[u8; ADDRESS_SIZE_BYTES]) -> Address {
        Address(Hash::from_bytes(data))
    }
}

/// Serializer for `Address`
#[derive(Default, Clone)]
pub struct AddressSerializer;

impl AddressSerializer {
    /// Serializes an `Address` into a `Vec<u8>`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<Address> for AddressSerializer {
    fn serialize(
        &self,
        value: &Address,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `Address`
#[derive(Default, Clone)]
pub struct AddressDeserializer {
    hash_deserializer: HashDeserializer,
}

impl AddressDeserializer {
    /// Creates a new deserializer for `Address`
    pub const fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<Address> for AddressDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_models::address::{Address, AddressDeserializer};
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let address = Address::from_str("A12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap();
    /// let bytes = address.into_bytes();
    /// let (rest, res_addr) = AddressDeserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Address, E> {
        context("Failed Address deserialization", |input| {
            self.hash_deserializer.deserialize(input)
        })
        .map(Address)
        .parse(buffer)
    }
}

/// Info for a given address on a given cycle
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionAddressCycleInfo {
    /// cycle number
    pub cycle: u64,
    /// true if that cycle is final
    pub is_final: bool,
    /// `ok_count` blocks were created by this address during that cycle
    pub ok_count: u64,
    /// `ok_count` blocks were missed by this address during that cycle
    pub nok_count: u64,
    /// number of active rolls the address had at that cycle (if still available)
    pub active_rolls: Option<u64>,
}
