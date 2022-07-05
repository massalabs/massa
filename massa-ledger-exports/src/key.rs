use massa_models::{Address, AddressDeserializer, VecU8Deserializer, VecU8Serializer};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use nom::error::{ContextError, ParseError};
use std::ops::Bound::Included;

pub const BALANCE_IDENT: u8 = 0u8;
pub const BYTECODE_IDENT: u8 = 1u8;
pub const DATASTORE_IDENT: u8 = 2u8;

/// Balance key formatting macro
#[macro_export]
macro_rules! balance_key {
    ($addr:expr) => {
        [&$addr.to_bytes()[..], &[BALANCE_IDENT]].concat()
    };
}

/// Bytecode key formatting macro
///
/// NOTE: still handle separate bytecode for now to avoid too many refactoring at once
#[macro_export]
macro_rules! bytecode_key {
    ($addr:expr) => {
        [&$addr.to_bytes()[..], &[BYTECODE_IDENT]].concat()
    };
}

/// Datastore entry key formatting macro
///
/// TODO: add a separator identifier if the need comes to have multiple datastores
#[macro_export]
macro_rules! data_key {
    ($addr:expr, $key:expr) => {
        [&$addr.to_bytes()[..], &[DATASTORE_IDENT], &$key].concat()
    };
}

/// Datastore entry prefix formatting macro
#[macro_export]
macro_rules! data_prefix {
    ($addr:expr) => {
        &[&$addr.to_bytes()[..], &[DATASTORE_IDENT]].concat()
    };
}

/// Extract an address from a key
pub fn get_address_from_key(key: &[u8]) -> Option<Address> {
    let address_deserializer = AddressDeserializer::new();
    address_deserializer
        .deserialize::<DeserializeError>(key)
        .map(|res| res.1)
        .ok()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    pub address: Address,
    pub ident: u8,
    pub store_key: Option<Vec<u8>>,
}

/// Basic key serializer
pub struct KeySerializer {
    vec_u8_serializer: VecU8Serializer,
}

impl KeySerializer {
    /// Creates a new `KeySerializer`
    pub fn new() -> Self {
        KeySerializer {
            vec_u8_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<Key> for KeySerializer {
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::{KeySerializer, Key};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let mut serialized = Vec::new();
    /// let address = Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Some(b"test".to_vec());
    /// let key = Key {
    ///     address,
    ///     ident: 2u8,
    ///     store_key,
    /// };
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        key: &Key,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend(key.address.to_bytes());
        buffer.extend([key.ident]);
        if let Some(value) = &key.store_key {
            self.vec_u8_serializer.serialize(value, buffer)?;
        }
        Ok(())
    }
}

/// Basic key deserializer
pub struct KeyDeserializer {
    address_deserializer: AddressDeserializer,
    vec_u8_deserializer: VecU8Deserializer,
}

impl Default for KeyDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyDeserializer {
    /// Creates a new `KeyDeserializer`
    pub fn new() -> Self {
        Self {
            address_deserializer: AddressDeserializer::new(),
            vec_u8_deserializer: VecU8Deserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

// TODO: deserialize keys into a rust type
impl Deserializer<Key> for KeyDeserializer {
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::{KeyDeserializer, KeySerializer, Key};
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let mut serialized = Vec::new();
    /// let address = Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Some(b"test".to_vec());
    /// let key = Key {
    ///     address,
    ///     ident: 2u8,
    ///     store_key,
    /// };
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new().deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(key_deser, key);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> nom::IResult<&'a [u8], Key, E> {
        let (rest, address) = self.address_deserializer.deserialize(buffer)?;
        let error = nom::Err::Error(ParseError::from_error_kind(
            buffer,
            nom::error::ErrorKind::Fail,
        ));
        match rest.first() {
            Some(ident) => match *ident {
                BALANCE_IDENT => Ok((
                    &rest[1..],
                    Key {
                        address,
                        ident: BALANCE_IDENT,
                        store_key: None,
                    },
                )),
                BYTECODE_IDENT => Ok((
                    &rest[1..],
                    Key {
                        address,
                        ident: BYTECODE_IDENT,
                        store_key: None,
                    },
                )),
                DATASTORE_IDENT => {
                    let (rest, key) = self.vec_u8_deserializer.deserialize(&rest[1..])?;
                    Ok((
                        &rest[1..],
                        Key {
                            address,
                            ident: BYTECODE_IDENT,
                            store_key: Some(key),
                        },
                    ))
                }
                _ => Err(error),
            },
            None => Err(error),
        }
    }
}
