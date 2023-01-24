// Copyright (c) 2022 MASSA LABS <info@massa.net>
mod sc_address;
use nom::character::complete::char;
use sc_address::*;
mod user_address;
pub use user_address::*;

use crate::error::ModelsError;
use crate::prehash::PreHashed;
use massa_hash::Hash;
use massa_serialization::{Deserializer, Serializer};
use massa_signature::PublicKey;
use nom::error::{context, ContextError, ParseError};
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

/// Size of a serialized address, in bytes
pub const ADDRESS_SIZE_BYTES: usize = massa_hash::HASH_SIZE_BYTES;

/// Captures the context of an address. For now, codebase assumes it's always User
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
pub enum Address {
    ///
    User(UserAddress),
    ///
    SC(SCAddress),
}

impl From<SCAddress> for Address {
    fn from(value: SCAddress) -> Self {
        Self::SC(value)
    }
}
impl From<UserAddress> for Address {
    fn from(value: UserAddress) -> Self {
        Self::User(value)
    }
}
const ADDRESS_PREFIX: u8 = b'A';

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}",
            ADDRESS_PREFIX,
            self.variant_prefix(),
            match self {
                Address::User(addr) => addr,
                Address::SC(_addr) => unimplemented!(),
            }
        )
    }
}

#[allow(missing_docs)]
pub trait AddressTrait {
    const PREFIX: u8;
    const VERSION: u64;
    #[allow(missing_docs)]
    fn get_thread(&self, thread_count: u8) -> u8;
    #[allow(missing_docs)]
    fn to_bytes(&self) -> &[u8];
    fn from_bytes(bytes: &[u8]) -> Result<Self, ()>
    where
        Self: Sized;
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
        let Some('A') = chars.next() else {
            return Err(ModelsError::AddressParseError);
        };
        let prefix = chars.next();
        let s = &s[2..];
        match prefix {
            Some('U') => Ok(Self::User(UserAddress::from_str(s)?)),
            Some('S') => Ok(Self::SC(
                SCAddress::from_bytes(s.as_bytes()).expect("todo: bubble up error"),
            )),
            Some(_) | None => Err(ModelsError::AddressParseError),
        }
    }
}

#[test]
fn test_address_str_format() {
    use massa_signature::KeyPair;

    let keypair = KeyPair::generate();
    let address = Address::from_public_key(&keypair.get_public_key());
    let a = address.to_string();
    dbg!(&a);
    let b = Address::from_str(&a).unwrap();
    assert_eq!(address, b);
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
        Self::User(UserAddress(Hash::compute_from(public_key.to_bytes())))
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
        let Address::User(addr) = self else {
            todo!("return result");
        };
        addr.0.to_bytes()
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
        let Address::User(addr) = self else {
            todo!("return result");
        };
        addr.0.into_bytes()
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
        Self::User(UserAddress(Hash::from_bytes(data)))
    }

    const fn variant_prefix(&self) -> char {
        match self {
            Address::User(_) => 'U',
            Address::SC(_) => 'S',
        }
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

impl<T: AddressTrait> Serializer<T> for AddressSerializer {
    fn serialize(
        &self,
        value: &T,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.push(b'A');
        buffer.push(T::PREFIX);
        buffer.extend_from_slice(value.to_bytes());
        Ok(())
    }
}

/// Deserializer for `Address`
#[derive(Clone)]
pub struct AddressDeserializer;

impl AddressDeserializer {
    /// Creates a new deserializer for `Address`
    pub const fn new() -> Self {
        Self
    }
}

impl<T: AddressTrait> Deserializer<T> for AddressDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_models::address::{UserAddress, Address, AddressDeserializer};
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let user_address = UserAddress::from_str("12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap();
    /// let address = Address::User(user_address);
    /// let bytes = b"AU0foobar";
    /// let (rest, res_addr) = AddressDeserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// assert_eq!(address, res_addr);
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], T, E> {
        let (rest, _) =
            context("Invalid Address Prefix", char(ADDRESS_PREFIX.into())).parse(buffer)?;
        let (rest, pref) =
            context("Invalid Address Variant Prefix", char(T::PREFIX.into())).parse(rest)?;

        let res = T::from_bytes(rest).map_err(|_| {
            nom::Err::Failure(ContextError::add_context(
                rest,
                "Invalid byte stream",
                ParseError::from_error_kind(rest, nom::error::ErrorKind::Fail),
            ))
        })?;
        Ok((rest, res))
    }
}

#[cfg(test)]
mod test {
    use massa_serialization::DeserializeError;

    use crate::slot::Slot;

    use super::*;
    #[test]
    fn sniff_sc_serde() {
        let addr: Address = SCAddress::new(Slot::new(0, 0), 0, true).into();
        let ser = AddressSerializer::new();
        let mut buf = Vec::new();
        ser.serialize(&addr, &mut buf).unwrap();
        dbg!(&buf);
        let deser: Address = AddressDeserializer::new()
            .deserialize::<DeserializeError>(&mut buf.as_slice())
            .unwrap()
            .1
            .into();
        assert_eq!(deser, addr);
    }
    #[test]
    fn sniff_user_serde() {
        let user_address =
            UserAddress::from_str("12hgh5ULW9o8fJE9muLNXhQENaUUswQbxPyDSq8ridnDGu5gRiJ").unwrap();
        let address = Address::User(user_address);
        let mut bytes = b"AU".to_vec();
        bytes.append(&mut user_address.clone().to_bytes().to_vec());
        let (rest, res_addr) = AddressDeserializer::new()
            .deserialize::<DeserializeError>(&bytes)
            .unwrap();
        assert_eq!(address, res_addr);
        assert_eq!(rest.len(), 0);
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
