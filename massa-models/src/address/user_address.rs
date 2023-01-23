use crate::error::ModelsError;
use massa_hash::{Hash, HashDeserializer};
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_signature::PublicKey;
use nom::error::{context, ContextError, ParseError};
use nom::{IResult, Parser};
use std::ops::Bound::Included;
use std::str::FromStr;

use super::AddressTrait;

/// Size of the decoded address hash, in bytes
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// Derived from a public key
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UserAddress(pub Hash);

impl AddressTrait for UserAddress {
    const PREFIX: char = 'U';
    const VERSION: u64 = 0;
    fn get_thread(&self, thread_count: u8) -> u8 {
        (self.to_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }
}
impl UserAddress {
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
    pub fn from_bytes(data: &[u8; ADDRESS_SIZE_BYTES]) -> Self {
        Self(Hash::from_bytes(data))
    }
}
impl From<&PublicKey> for UserAddress {
    fn from(value: &PublicKey) -> Self {
        Self(Hash::compute_from(value.to_bytes()))
    }
}

impl std::fmt::Display for UserAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        // might want to allocate the vector with capacity in order to avoid re-allocation
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&Self::VERSION, &mut bytes)
            .map_err(|_| std::fmt::Error)?;
        bytes.extend(self.0.to_bytes());
        write!(f, "{}", bs58::encode(bytes).with_check().into_string())
    }
}

impl<'de> ::serde::Deserialize<'de> for UserAddress {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<UserAddress, D::Error> {
        if d.is_human_readable() {
            struct StringVisitor;

            impl<'de> ::serde::de::Visitor<'de> for StringVisitor {
                type Value = UserAddress;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("A + variant prefix + inner address")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        Self::Value::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Self::Value::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(StringVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = UserAddress;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("a bytestring")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    Ok(Self::Value::from_bytes(v.try_into().map_err(E::custom)?))
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}
impl FromStr for UserAddress {
    type Err = ModelsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let decoded_bs58_check = bs58::decode(s)
            .with_check(None)
            .into_vec()
            .map_err(|_| ModelsError::AddressParseError)?;
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, _version) = u64_deserializer
            .deserialize::<DeserializeError>(&decoded_bs58_check[..])
            .map_err(|_| ModelsError::AddressParseError)?;
        Ok(UserAddress(Hash::from_bytes(
            rest.try_into()
                .map_err(|_| ModelsError::AddressParseError)?,
        )))
    }
}

impl std::fmt::Debug for UserAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
    }
}

impl ::serde::Serialize for UserAddress {
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_string())
        } else {
            s.serialize_bytes(self.to_bytes())
        }
    }
}

/// Serializer for `Address`
#[derive(Default, Clone)]
pub struct UserAddressSerializer;

impl UserAddressSerializer {
    /// Serializes an `Address` into a `Vec<u8>`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<UserAddress> for UserAddressSerializer {
    fn serialize(
        &self,
        value: &UserAddress,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend_from_slice(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `Address`
#[derive(Default, Clone)]
pub struct UserAddressDeserializer {
    hash_deserializer: HashDeserializer,
}

impl UserAddressDeserializer {
    /// Creates a new deserializer for `Address`
    pub const fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<UserAddress> for UserAddressDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_models::address::{UserAddress, Address, AddressDeserializer};
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let user_address = UserAddress::from_str("12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap();
    /// let address = Address::User(user_address);
    /// let bytes = address.into_bytes();
    /// let (rest, res_addr) = AddressDeserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], UserAddress, E> {
        context("Failed Address deserialization", |input| {
            self.hash_deserializer.deserialize(input)
        })
        .map(UserAddress)
        .parse(buffer)
    }
}

#[test]
fn test_address_str_format() {
    use massa_signature::KeyPair;

    let keypair = KeyPair::generate();
    let address = UserAddress::from(&keypair.get_public_key());
    let a = address.to_string();
    dbg!(&a);
    let b = UserAddress::from_str(&a).unwrap();
    assert_eq!(address, b);
}
