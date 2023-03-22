// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::prehash::PreHashed;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::PublicKey;
use nom::branch::alt;
use nom::combinator::verify;
use nom::error::{context, ContextError, ParseError};
use nom::sequence::preceded;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;
use std::str::FromStr;

/// Size of a serialized address, in bytes
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES + 1;

/// Top level address representation that can differentiate between User and SC address
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Address {
    #[allow(missing_docs)]
    User(UserAddress),
    #[allow(missing_docs)]
    SC(SCAddress),
}

/// In the near future, this will encapsulate slot, idx, and is_write
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SCAddress(pub Hash);

/// Derived from a public key.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UserAddress(pub Hash);

/// TODO: This conversion will need re-writing/removal when SCAddress is optimised
impl From<SCAddress> for UserAddress {
    fn from(value: SCAddress) -> Self {
        Self(value.0)
    }
}
/// TODO: This conversion will need re-writing/removal when SCAddress is optimised
impl From<UserAddress> for SCAddress {
    fn from(value: UserAddress) -> Self {
        Self(value.0)
    }
}

const ADDRESS_PREFIX: char = 'A';
// serialized with varint
const USER_PREFIX: u64 = 0;
const SC_PREFIX: u64 = 1;
const ADDRESS_VERSION: u64 = 0;

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&ADDRESS_VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.hash_bytes());
        write!(
            f,
            "{}{}{}",
            ADDRESS_PREFIX,
            match self {
                Address::User(_) => 'U',
                Address::SC(_) => 'S',
            },
            bs58::encode(bytes).with_check().into_string()
        )
    }
}

// See https://github.com/massalabs/massa/pull/3479#issuecomment-1408694720
// as to why more information is not provided
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
            s.serialize_bytes(&self.prefixed_bytes())
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
                    formatter.write_str("A + {U | S} + base58::encode(version + hash)")
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
                    formatter.write_str("[u64varint-of-addr-variant][u64varint-of-version][bytes]")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    AddressDeserializer::new()
                        .deserialize::<DeserializeError>(v)
                        .map_err(E::custom)
                        .map(|r| r.1)
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
        let err = Err(ModelsError::AddressParseError(s.to_string()));

        // Handle the prefix ("A{U|S}")
        let mut chars = s.chars();
        let Some(ADDRESS_PREFIX) = chars.next() else {
            return err;
        };
        let Some(pref) = chars.next() else {
            return err;
        };

        // Turn the version + hash encoded string into a byte-vec
        let data = chars.collect::<String>();
        let decoded_bs58_check = bs58::decode(data)
            .with_check(None)
            .into_vec()
            .map_err(|_| ModelsError::AddressParseError(s.to_string()))?;

        // extract the version
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, _version) = u64_deserializer
            .deserialize::<DeserializeError>(&decoded_bs58_check[..])
            .map_err(|_| ModelsError::AddressParseError(s.to_string()))?;

        // ...and package it up
        let res = UserAddress(Hash::from_bytes(
            rest.try_into()
                .map_err(|_| ModelsError::AddressParseError(s.to_string()))?,
        ));

        let res = match pref {
            'U' => Address::User(res),
            'S' => Address::SC(res.into()),
            _ => return err,
        };
        Ok(res)
    }
}

impl PreHashed for Address {}

impl Address {
    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.hash_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }

    fn hash_bytes(&self) -> &[u8; 32] {
        match self {
            Address::User(addr) => addr.0.to_bytes(),
            Address::SC(addr) => addr.0.to_bytes(),
        }
    }

    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Address::User(UserAddress(Hash::compute_from(public_key.to_bytes())))
    }

    /// Inner implementation for serializer. Mostly made available for the benefit of macros.
    pub fn prefixed_bytes(&self) -> Vec<u8> {
        let mut buff = vec![];
        let pref_ser = U64VarIntSerializer::new();
        let val = match self {
            Address::User(_) => USER_PREFIX,
            Address::SC(_) => SC_PREFIX,
        };
        pref_ser
            .serialize(&val, &mut buff)
            .expect("impl always returns Ok(())");
        buff.extend_from_slice(&self.hash_bytes()[..]);
        buff
    }

    #[cfg(any(test, feature = "testing"))]
    /// Convenience wrapper around the address serializer. Useful for hard-coding an address when testing
    pub fn from_prefixed_bytes(data: &[u8]) -> Result<Address, ModelsError> {
        let deser = AddressDeserializer::new();
        let (_, res) = deser.deserialize::<DeserializeError>(data).map_err(|_| {
            match std::str::from_utf8(data) {
                Ok(res) => ModelsError::AddressParseError(res.to_string()),
                Err(e) => {
                    ModelsError::AddressParseError(format!("Error on retrieve address : {}", e))
                }
            }
        })?;
        Ok(res)
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
    /// # Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::{UserAddress, Address, AddressSerializer};
    /// # use massa_serialization::{Serializer, SerializeError};
    /// use massa_hash::Hash;
    /// let bytes = &[0; 32];
    /// // Make a hard-coded 0-byte container
    /// let ref_addr = Address::User(UserAddress(Hash::from_bytes(bytes)));
    /// let mut vec = vec![];
    /// AddressSerializer::new().serialize(&ref_addr, &mut vec).unwrap();
    /// // the deser adds the prefix value '0' in a single byte
    /// assert_eq!(vec, [[0].as_slice(), bytes.as_slice()].concat());
    /// ```
    fn serialize(
        &self,
        value: &Address,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(&value.prefixed_bytes());
        Ok(())
    }
}

/// Deserializer for `Address`
#[derive(Clone)]
pub struct AddressDeserializer {
    hash_deserializer: HashDeserializer,
    int_deserializer: U64VarIntDeserializer,
}
impl AddressDeserializer {
    /// Creates a new deserializer for `Address`
    pub const fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
            int_deserializer: U64VarIntDeserializer::new(Included(0), Included(1)),
        }
    }
}

impl Deserializer<Address> for AddressDeserializer {
    /// # Example
    /// ```rust
    /// use massa_models::address::{UserAddress, Address, AddressDeserializer};
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use massa_hash::Hash;
    /// // Make a hard-coded 0-byte container
    /// let bytes = [[0].as_slice(), [0; 32].as_slice()].concat();
    /// let ref_addr = Address::User(UserAddress(Hash::from_bytes(bytes[1..33].try_into().unwrap())));
    /// let res_addr = AddressDeserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// // the deser adds the prefix value '0' in a single byte
    /// assert_eq!(ref_addr, res_addr.1);
    /// assert_eq!(0, res_addr.0.len());
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Address, E> {
        context("Address Variant", |input| {
            alt((
                |input| user_parser(&self.int_deserializer, &self.hash_deserializer, input),
                |input| sc_parser(&self.int_deserializer, &self.hash_deserializer, input),
            ))
            .parse(input)
        })
        .parse(buffer)
    }
}

// used to make the `alt(...)` more readable
fn user_parser<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
    pref_deser: &U64VarIntDeserializer,
    deser: &HashDeserializer,
    input: &'a [u8],
) -> IResult<&'a [u8], Address, E> {
    context(
        "Failed attempt to deserialise User Address",
        preceded(
            verify(
                |input| pref_deser.deserialize(input),
                |val| *val == USER_PREFIX,
            ),
            |input| deser.deserialize(input),
        ),
    )
    .map(|hash| Address::User(UserAddress(hash)))
    .parse(input)
}
// used to make the `alt(...)` more readable. Will be usefull when the SCAddress will be deserialised differently
fn sc_parser<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
    pref_deser: &U64VarIntDeserializer,
    deser: &HashDeserializer,
    input: &'a [u8],
) -> IResult<&'a [u8], Address, E> {
    context(
        "Failed attempt to deserialise SC Address",
        preceded(
            verify(
                |input| pref_deser.deserialize(input),
                |val| *val == SC_PREFIX,
            ),
            |input| deser.deserialize(input),
        ),
    )
    .map(|hash| Address::SC(SCAddress(hash)))
    .parse(input)
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_address_str_format() {
        use massa_signature::KeyPair;

        let keypair = KeyPair::generate();
        let address = Address::from_public_key(&keypair.get_public_key());
        let a = address.to_string();
        let b = Address::from_str(&a).unwrap();
        assert_eq!(address, b);
    }
}
