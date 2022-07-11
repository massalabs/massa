use massa_hash::HashDeserializer;
use massa_models::{address::AddressDeserializer, Address};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use nom::error::{ContextError, ParseError};

pub const SEQ_BALANCE_IDENT: u8 = 0u8;
pub const PAR_BALANCE_IDENT: u8 = 1u8;
pub const BYTECODE_IDENT: u8 = 2u8;
pub const DATASTORE_IDENT: u8 = 3u8;

/// Sequential balance key formatting macro
#[macro_export]
macro_rules! seq_balance_key {
    ($addr:expr) => {
        [&$addr.to_bytes()[..], &[SEQ_BALANCE_IDENT]].concat()
    };
}

/// Parallel balance key formatting macro
#[macro_export]
macro_rules! par_balance_key {
    ($addr:expr) => {
        [&$addr.to_bytes()[..], &[PAR_BALANCE_IDENT]].concat()
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
        [
            &$addr.to_bytes()[..],
            &[DATASTORE_IDENT],
            &$key.to_bytes()[..],
        ]
        .concat()
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

/// Basic key serializer
#[derive(Default)]
pub struct KeySerializer;

impl KeySerializer {
    /// Creates a new `KeySerializer`
    pub fn new() -> Self {
        Self
    }
}

impl Serializer<Vec<u8>> for KeySerializer {
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::KeySerializer;
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let mut serialized = Vec::new();
    /// let address = Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Hash::compute_from(b"test");
    /// let mut key = Vec::new();
    /// key.extend(address.to_bytes());
    /// key.push(2u8);
    /// key.extend(store_key.to_bytes());
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &Vec<u8>,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        buffer.extend(value);
        Ok(())
    }
}

/// Basic key deserializer
#[derive(Default)]
pub struct KeyDeserializer {
    address_deserializer: AddressDeserializer,
    hash_deserializer: HashDeserializer,
}

impl KeyDeserializer {
    /// Creates a new `KeyDeserializer`
    pub fn new() -> Self {
        Self {
            address_deserializer: AddressDeserializer::new(),
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

// TODO: deserialize keys into a rust type
impl Deserializer<Vec<u8>> for KeyDeserializer {
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::{KeyDeserializer, KeySerializer};
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let mut serialized = Vec::new();
    /// let address = Address::from_str("A12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Hash::compute_from(b"test");
    /// let mut key = Vec::new();
    /// key.extend(address.to_bytes());
    /// key.push(2u8);
    /// key.extend(store_key.to_bytes());
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new().deserialize::<DeserializeError>(&serialized).unwrap();
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
        match rest.first() {
            Some(ident) => match *ident {
                SEQ_BALANCE_IDENT => Ok((&rest[1..], seq_balance_key!(address))),
                PAR_BALANCE_IDENT => Ok((&rest[1..], par_balance_key!(address))),
                BYTECODE_IDENT => Ok((&rest[1..], bytecode_key!(address))),
                DATASTORE_IDENT => {
                    let (rest, hash) = self.hash_deserializer.deserialize(&rest[1..])?;
                    Ok((rest, data_key!(address, hash)))
                }
                _ => Err(error),
            },
            None => Err(error),
        }
    }
}
