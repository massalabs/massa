// Copyright (c) 2022 MASSA LABS <info@massa.net>

use crate::error::ModelsError;
use crate::prehash::PreHashed;
use massa_hash::{Hash, HashDeserializer, HASH_SIZE_BYTES};
use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use massa_signature::PublicKey;
use nom::error::{context, ContextError, ErrorKind, ParseError};
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use sha2::Digest;
use sha2::Sha512;
use std::ops::Bound::{Excluded, Included};
use std::str::FromStr;
use transition::Versioned;

/// Derived from a public key
#[transition::versioned(versions("1", "2"))]
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Address(pub Hash);

const ADDRESS_PREFIX: char = 'A';

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        //CallVersions!(self, fmt(f));
        //TODO: https://stackoverflow.com/questions/75171139/use-macro-in-match-branch
        match self {
            Address::AddressV1(address) => address.fmt(f),
            Address::AddressV2(address) => address.fmt(f)
        }
    }
}

impl ::serde::Serialize for Address {
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        //CallVersions!(self, serialize(s));
        //TODO: https://stackoverflow.com/questions/75171139/use-macro-in-match-branch
        match self {
            Address::AddressV1(address) => address.serialize(s),
            Address::AddressV2(address) => address.serialize(s),
        }
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self)
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
                    Address::from_bytes(v).map_err(E::custom)
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
    /// # let keypair = KeyPair::generate(1).unwrap();
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
                let (rest, version) = u64_deserializer
                    .deserialize::<DeserializeError>(&decoded_bs58_check[..])
                    .map_err(|_| ModelsError::AddressParseError)?;
                //TODO: Make it function like macro from address big structure
                match version {
                    <Address!["1"]>::VERSION => Ok(AddressVariant!["1"](<Address!["1"]>::from_bytes(
                        rest.try_into()
                            .map_err(|_| ModelsError::AddressParseError)?,
                    )?)),
                    <Address!["1"]>::VERSION => Ok(AddressVariant!["2"](<Address!["2"]>::from_bytes(
                        rest.try_into()
                            .map_err(|_| ModelsError::AddressParseError)?,
                    )?)),
                    _ => Err(ModelsError::AddressParseError),
                }
            }
            _ => Err(ModelsError::AddressParseError),
        }
    }
}

impl Address {
    pub fn from_bytes(data: &[u8]) -> Result<Self, ModelsError> {
        let u64_deserializer = U64VarIntDeserializer::new(Included(0), Included(u64::MAX));
        let (rest, version) = u64_deserializer
            .deserialize::<DeserializeError>(&data[..])
            .map_err(|_| ModelsError::AddressParseError)?;
        // BuildVersions!(
        //     version,
        //     from_bytes_without_version(rest.try_into().map_err(|_| ModelsError::AddressParseError)?)?
        //     ModelsError::InvalidVersionError(format!("Address version {} doesn't exist.", version))
        // );
        match version {
            <Address!["1"]>::VERSION => Ok(AddressVariant!["1"](
                <Address!["1"]>::from_bytes_without_version(
                    rest.try_into()
                        .map_err(|_| ModelsError::AddressParseError)?,
                )?,
            )),
            <Address!["2"]>::VERSION => Ok(AddressVariant!["2"](
                <Address!["2"]>::from_bytes_without_version(
                    rest.try_into()
                        .map_err(|_| ModelsError::AddressParseError)?,
                )?,
            )),
        }
    }

    pub fn into_bytes(&self) -> Vec<u8> {
        //CallVersions!(self, into_bytes());
        //TODO: https://stackoverflow.com/questions/75171139/use-macro-in-match-branch
        match self {
            Address::AddressV1(a) => a.into_bytes(),
            Address::AddressV2(a) => a.into_bytes(),
        }
    }

    pub fn from_public_key(version: u64, public_key: &PublicKey) -> Result<Self, ModelsError> {
        //BuildVersions!(version, from_public_key(public_key), ModelsError::InvalidVersionError(format!("Address version {} doesn't exist.", version)));
        match version {
            <Address!["1"]>::VERSION => Ok(AddressVariant!["1"](<Address!["1"]>::from_public_key(
                public_key,
            ))),
            <Address!["2"]>::VERSION => Ok(AddressVariant!["2"](<Address!["2"]>::from_public_key(
                public_key,
            ))),
            _ => Err(ModelsError::InvalidVersionError(format!(
                "Address version {} doesn't exist.",
                version
            ))),
        }
    }
}

impl PreHashed for Address {}

#[test]
fn test_address_str_format() {
    use massa_signature::KeyPair;

    let keypair = KeyPair::generate(1).unwrap();
    let address = Address::from_public_key(1, &keypair.get_public_key()).unwrap();
    let a = address.to_string();
    let b = Address::from_str(&a).unwrap();
    assert!(address == b);
}

#[transition::impl_version(versions("1", "2"))]
impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let u64_serializer = U64VarIntSerializer::new();
        let mut bytes: Vec<u8> = Vec::new();
        u64_serializer
            .serialize(&Self::VERSION, &mut bytes)
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

#[transition::impl_version(versions("1", "2"))]
impl ::serde::Serialize for Address {
    fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(&self.to_string())
        } else {
            s.serialize_bytes(&self.into_bytes())
        }
    }
}

#[transition::impl_version(versions("1"), structures("Address"))]
impl Address {
    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Address(Hash::compute_from(&public_key.to_bytes()))
    }

    pub const SIZE_BYTES: usize = HASH_SIZE_BYTES + Self::VERSION_VARINT_SIZE_BYTES;
}

#[transition::impl_version(versions("2"), structures("Address"))]
impl Address {
    /// Computes address associated with given public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Address(Hash::from_bytes(
            &Sha512::digest(public_key.to_bytes())[..]
                .try_into()
                .unwrap(),
        ))
    }

    // Should be in common but it's to showcase the potential of the macro
    pub const SIZE_BYTES: usize = HASH_SIZE_BYTES + Self::VERSION_VARINT_SIZE_BYTES;
}

#[transition::impl_version(versions("1", "2"), structures("Address"))]
impl Address {

    pub fn get_version(&self) -> u64 {
        Self::VERSION
    }

    /// Gets the associated thread. Depends on the `thread_count`
    pub fn get_thread(&self, thread_count: u8) -> u8 {
        (self.into_bytes()[0])
            .checked_shr(8 - thread_count.trailing_zeros())
            .unwrap_or(0)
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate(1).unwrap();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = address.into_bytes();
    /// let res_addr = Address::from_bytes(&bytes);
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn into_bytes(self) -> Vec<u8> {
        let version_serializer = U64VarIntSerializer::new();
        let mut bytes: Vec<u8> = Vec::new();
        version_serializer.serialize(&Self::VERSION, &mut bytes);
        bytes[Self::VERSION_VARINT_SIZE_BYTES..].copy_from_slice(&self.0.into_bytes());
        bytes
    }

    /// ## Example
    /// ```rust
    /// # use massa_signature::{PublicKey, KeyPair, Signature};
    /// # use massa_hash::Hash;
    /// # use serde::{Deserialize, Serialize};
    /// # use massa_models::address::Address;
    /// # let keypair = KeyPair::generate(1).unwrap();
    /// # let address = Address::from_public_key(&keypair.get_public_key());
    /// let bytes = address.to_bytes();
    /// let res_addr = Address::from_bytes(&bytes);
    /// assert_eq!(address, res_addr);
    /// ```
    pub fn from_bytes(data: &[u8; Self::SIZE_BYTES]) -> Result<Address, ModelsError> {
        let version_deserializer =
            U64VarIntDeserializer::new(Included(Self::VERSION), Excluded(Self::VERSION + 1));
        let (rest, version) = version_deserializer.deserialize(data)?;
        if version != Self::VERSION {
            return Err(ModelsError::InvalidVersionError(format!(
                "Expected address version {}, got {}",
                Self::VERSION,
                version
            )));
        } else {
            Ok(Address::from_bytes_without_version(rest)?)
        }
    }

    fn from_bytes_without_version(data: &[u8]) -> Result<Address, ModelsError> {
        Ok(Address(Hash::from_bytes(&data.try_into().map_err(
            |_| {
                ModelsError::BufferError(format!(
                    "expected a buffer of size {}, but found a size of {}",
                    HASH_SIZE_BYTES,
                    &data.len()
                ))
            },
        )?)))
    }
}

/// Serializer for `Address`
#[derive(Default, Clone)]
pub struct AddressSerializer {
    version_serializer: U64VarIntSerializer,
}

impl AddressSerializer {
    /// Serializes an `Address` into a `Vec<u8>`
    pub fn new() -> Self {
        Self {
            version_serializer: U64VarIntSerializer::new()
        }
    }
}

impl Serializer<Address> for AddressSerializer {
    fn serialize(&self, value: &Address, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            Address::AddressV1(addr) => self.serialize(addr, buffer),
            Address::AddressV2(addr) => self.serialize(addr, buffer),
        }
    }
}

#[transition::impl_version(versions("1", "2"), structures("Address"))]
impl Serializer<Address> for AddressSerializer {
    fn serialize(&self, value: &Address, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.version_serializer.serialize(&value.get_version(), buffer)?;
        buffer.extend_from_slice(&value.into_bytes());
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
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Address, E> {
        if buffer.len() < 2 {
            return Err(nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof)));
        }
        let version_byte = buffer[0];
        match version_byte {
            1 => {
                let (rest, addr) = self.deserialize(&buffer[1..])?;
                Ok((rest, AddressVariant!["1"](addr)))
            }
            2 => {
                let (rest, addr) = self.deserialize(&buffer[1..])?;
                Ok((rest, AddressVariant!["2"](addr)))
            }
            _ => Err(nom::Err::Error(E::from_error_kind(buffer, ErrorKind::Eof))),
        }
    }
}

#[transition::impl_version(versions("1", "2"), structures("Address"))]
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
