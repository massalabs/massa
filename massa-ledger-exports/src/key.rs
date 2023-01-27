use massa_models::{
    address::{Address, AddressDeserializer, ADDRESS_SIZE_BYTES},
    serialization::{VecU8Deserializer, VecU8Serializer},
};
use massa_serialization::{DeserializeError, Deserializer, SerializeError, Serializer};
use nom::error::{ContextError, ParseError};
use std::ops::Bound::Included;

pub const BALANCE_IDENT: u8 = 0u8;
pub const BYTECODE_IDENT: u8 = 1u8;
pub const DATASTORE_IDENT: u8 = 2u8;

/// Balance key formatting macro
#[macro_export]
macro_rules! balance_key {
    ($addr:expr) => {
        [&$addr.prefixed_bytes()[..], &[BALANCE_IDENT]].concat()
    };
}

/// Bytecode key formatting macro
///
/// NOTE: still handle separate bytecode for now to avoid too many refactor at once
#[macro_export]
macro_rules! bytecode_key {
    ($addr:expr) => {
        [&$addr.prefixed_bytes()[..], &[BYTECODE_IDENT]].concat()
    };
}

/// Datastore entry key formatting macro
///
/// TODO: add a separator identifier if the need comes to have multiple datastore
#[macro_export]
macro_rules! data_key {
    ($addr:expr, $key:expr) => {
        [&$addr.prefixed_bytes()[..], &[DATASTORE_IDENT], &$key].concat()
    };
}

/// Datastore entry prefix formatting macro
#[macro_export]
macro_rules! data_prefix {
    ($addr:expr) => {
        &[&$addr.prefixed_bytes()[..], &[DATASTORE_IDENT]].concat()
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

/// Basic key serializer
#[derive(Default, Clone)]
pub struct KeySerializer {
    vec_u8_serializer: VecU8Serializer,
}

impl KeySerializer {
    /// Creates a new `KeySerializer`
    pub fn new() -> Self {
        Self {
            vec_u8_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<Vec<u8>> for KeySerializer {
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::{KeySerializer, DATASTORE_IDENT};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let mut serialized = Vec::new();
    /// let address = Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Hash::compute_from(b"test");
    /// let mut key = Vec::new();
    /// key.extend(address.prefixed_bytes());
    /// key.push(DATASTORE_IDENT);
    /// key.extend(store_key.to_bytes());
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &Vec<u8>, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        let limit = ADDRESS_SIZE_BYTES + 1;
        buffer.extend(&value[..limit]);
        if value[limit - 1] == DATASTORE_IDENT {
            if value.len() > limit {
                self.vec_u8_serializer
                    .serialize(&value[limit..].to_vec(), buffer)?;
            } else {
                return Err(SerializeError::GeneralError(
                    "datastore keys can not be empty".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// Basic key deserializer
#[derive(Clone)]
pub struct KeyDeserializer {
    address_deserializer: AddressDeserializer,
    datastore_key_deserializer: VecU8Deserializer,
}

impl KeyDeserializer {
    /// Creates a new `KeyDeserializer`
    pub fn new(max_datastore_key_length: u8) -> Self {
        Self {
            address_deserializer: AddressDeserializer::new(),
            datastore_key_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_key_length as u64),
            ),
        }
    }
}

// TODO: deserialize keys into a rust type
impl Deserializer<Vec<u8>> for KeyDeserializer {
    /// ## Example
    /// ```
    /// use massa_models::address::{AddressSerializer, Address };
    /// use massa_ledger_exports::{KeyDeserializer, KeySerializer, DATASTORE_IDENT, BALANCE_IDENT};
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let address = Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Hash::compute_from(b"test");
    ///
    /// let mut key = Vec::new();
    /// let mut serialized = Vec::new();
    /// key.extend(address.prefixed_bytes());
    /// key.push(DATASTORE_IDENT);
    /// key.extend(store_key.to_bytes());
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new(255).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(key_deser, key);
    ///
    /// let mut key = Vec::new();
    /// let mut serialized = Vec::new();
    /// key.extend(address.prefixed_bytes());
    /// key.push(BALANCE_IDENT);
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new(255).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(key_deser, key);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> nom::IResult<&'a [u8], Vec<u8>, E> {
        let (rest, address) = self.address_deserializer.deserialize(buffer)?;
        let error = nom::Err::Error(ParseError::from_error_kind(
            buffer,
            nom::error::ErrorKind::Fail,
        ));
        let Some(ident) = rest.first() else {
            return Err(error);
        };

        match *ident {
            BALANCE_IDENT => Ok((&rest[1..], balance_key!(address))),
            BYTECODE_IDENT => Ok((&rest[1..], bytecode_key!(address))),
            DATASTORE_IDENT => {
                let (rest, hash) = self.datastore_key_deserializer.deserialize(&rest[1..])?;
                Ok((rest, data_key!(address, hash)))
            }
            _ => Err(error),
        }
    }
}
