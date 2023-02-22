use massa_models::{
    address::{Address, AddressDeserializer, AddressSerializer},
    serialization::{VecU8Deserializer, VecU8Serializer},
};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::error::{ContextError, ParseError};
use std::ops::Bound::Included;

pub const BALANCE_IDENT: u8 = 0u8;
pub const BYTECODE_IDENT: u8 = 1u8;
pub const DATASTORE_IDENT: u8 = 2u8;

#[repr(u8)]
#[derive(PartialEq, Eq, Clone, Debug)]
pub enum KeyType {
    BALANCE = 0,
    BYTECODE = 1,
    DATASTORE(Vec<u8>) = 2,
}

impl KeyType {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(KeyType::BALANCE),
            1 => Some(KeyType::BYTECODE),
            2 => Some(KeyType::DATASTORE(Vec::new())),
            _ => None,
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            KeyType::BALANCE => 0,
            KeyType::BYTECODE => 1,
            KeyType::DATASTORE(_) => 2,
        }
    }
}

/// Disk ledger keys representation
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Key {
    pub key_type: KeyType,
    pub address: Address,
}

impl Key {
    pub fn new(address: &Address, key_type: KeyType) -> Self {
        Self {
            key_type,
            address: *address,
        }
    }
}

pub fn datastore_prefix_from_address(address: &Address) -> Vec<u8> {
    let mut prefix = Vec::new();
    AddressSerializer::new()
        .serialize(address, &mut prefix)
        .unwrap();
    prefix.extend([DATASTORE_IDENT]);
    prefix
}

/// Basic key serializer
#[derive(Default, Clone)]
pub struct KeySerializer {
    address_serializer: AddressSerializer,
    vec_u8_serializer: VecU8Serializer,
}

impl KeySerializer {
    /// Creates a new `KeySerializer`
    pub fn new() -> Self {
        Self {
            address_serializer: AddressSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<Key> for KeySerializer {
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::{KeySerializer, KeyType, Key};
    /// use massa_serialization::Serializer;
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let mut serialized = Vec::new();
    /// let address = Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Hash::compute_from(b"test");
    /// let mut key = Key::new(&address, KeyType::DATASTORE(store_key.into_bytes().to_vec()));
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &Key, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.address_serializer.serialize(&value.address, buffer)?;
        buffer.extend(&[value.key_type.clone().to_u8()]);

        if let KeyType::DATASTORE(data) = value.key_type.clone() {
            self.vec_u8_serializer.serialize(&data, buffer)?;
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
impl Deserializer<Key> for KeyDeserializer {
    /// ## Example
    /// ```
    /// use massa_models::address::Address;
    /// use massa_ledger_exports::{KeyDeserializer, KeySerializer, DATASTORE_IDENT, BALANCE_IDENT, KeyType, Key};
    /// use massa_serialization::{Deserializer, Serializer, DeserializeError};
    /// use massa_hash::Hash;
    /// use std::str::FromStr;
    ///
    /// let address = Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap();
    /// let store_key = Hash::compute_from(b"test");
    ///
    /// let mut key = Key::new(&address, KeyType::DATASTORE(store_key.into_bytes().to_vec()));
    /// let mut serialized = Vec::new();
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new(255).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(key_deser, key);
    ///
    /// let mut key = Key::new(&address, KeyType::BALANCE);
    /// let mut serialized = Vec::new();
    /// KeySerializer::new().serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new(255).deserialize::<DeserializeError>(&serialized).unwrap();
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
            Some(indent) => match KeyType::from_u8(*indent) {
                Some(KeyType::BALANCE) => Ok((
                    &rest[1..],
                    Key {
                        address,
                        key_type: KeyType::BALANCE,
                    },
                )),
                Some(KeyType::BYTECODE) => Ok((
                    &rest[1..],
                    Key {
                        address,
                        key_type: KeyType::BYTECODE,
                    },
                )),
                Some(KeyType::DATASTORE(_)) => {
                    println!("rest: {:?}", &rest[1..]);
                    let (rest, hash) = self.datastore_key_deserializer.deserialize(&rest[1..])?;
                    Ok((
                        rest,
                        Key {
                            address,
                            key_type: KeyType::DATASTORE(hash),
                        },
                    ))
                }
                _ => Err(error),
            },
            None => Err(error),
        }
    }
}
