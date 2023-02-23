use massa_models::{
    address::{Address, AddressDeserializer, AddressSerializer},
    serialization::{VecU8Deserializer, VecU8Serializer},
};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::error::{ContextError, ParseError};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

pub const BALANCE_IDENT: u8 = 0u8;
pub const BYTECODE_IDENT: u8 = 1u8;
pub const DATASTORE_IDENT: u8 = 2u8;

#[derive(PartialEq, Eq, Clone, IntoPrimitive, TryFromPrimitive, Debug)]
#[repr(u8)]
enum KeyTypeId {
    Balance = 0,
    Bytecode = 1,
    Datastore = 2,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum KeyType {
    BALANCE,
    BYTECODE,
    DATASTORE(Vec<u8>),
}

#[derive(Default, Clone)]
pub struct KeyTypeSerializer {
    vec_u8_serializer: VecU8Serializer,
    // Wether is deserialized with VecU8Deserializer or not.
    // This allows us to store the datastore key length at the beginning of the key or not.
    // The datastore key length is useful when transfering multiple keys, like in packets,
    // but isn't when storing a datastore key in the ledger.
    datastore_key_length: bool,
}

impl KeyTypeSerializer {
    pub fn new(datastore_key_length: bool) -> Self {
        Self {
            vec_u8_serializer: VecU8Serializer::new(),
            datastore_key_length,
        }
    }
}

impl Serializer<KeyType> for KeyTypeSerializer {
    fn serialize(&self, value: &KeyType, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            KeyType::BALANCE => buffer.extend(&[u8::from(KeyTypeId::Balance)]),
            KeyType::BYTECODE => buffer.extend(&[u8::from(KeyTypeId::Bytecode)]),
            KeyType::DATASTORE(data) => {
                buffer.extend(&[u8::from(KeyTypeId::Datastore)]);
                if self.datastore_key_length {
                    self.vec_u8_serializer.serialize(data, buffer)?;
                } else {
                    buffer.extend(data);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct KeyTypeDeserializer {
    vec_u8_deserializer: VecU8Deserializer,
    // Wether is deserialized with VecU8Deserializer or not.
    // This allows us to store the datastore key length at the beginning of the key or not.
    // The datastore key length is useful when transfering multiple keys, like in packets,
    // but isn't when storing a datastore key in the ledger.
    datastore_key_length: bool,
}

impl KeyTypeDeserializer {
    pub fn new(max_datastore_key_length: u8, datastore_key_length: bool) -> Self {
        Self {
            vec_u8_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_key_length as u64),
            ),
            datastore_key_length,
        }
    }
}

impl Deserializer<KeyType> for KeyTypeDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        input: &'a [u8],
    ) -> nom::IResult<&'a [u8], KeyType, E> {
        let (rest, key_type) = nom::number::complete::le_u8(input)?;
        match KeyTypeId::try_from(key_type) {
            Ok(KeyTypeId::Balance) => Ok((rest, KeyType::BALANCE)),
            Ok(KeyTypeId::Bytecode) => Ok((rest, KeyType::BYTECODE)),
            Ok(KeyTypeId::Datastore) => {
                if self.datastore_key_length {
                    let (rest, data) = self.vec_u8_deserializer.deserialize(rest)?;
                    Ok((rest, KeyType::DATASTORE(data)))
                } else {
                    Ok((&[], KeyType::DATASTORE(rest.to_vec())))
                }
            }
            Err(_) => Err(nom::Err::Error(E::from_error_kind(
                rest,
                nom::error::ErrorKind::Tag,
            ))),
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
    key_type_serializer: KeyTypeSerializer,
}

impl KeySerializer {
    /// Creates a new `KeySerializer`
    pub fn new(datastore_key_length: bool) -> Self {
        Self {
            address_serializer: AddressSerializer::new(),
            key_type_serializer: KeyTypeSerializer::new(datastore_key_length),
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
        self.key_type_serializer
            .serialize(&value.key_type, buffer)?;

        Ok(())
    }
}

/// Basic key deserializer
#[derive(Clone)]
pub struct KeyDeserializer {
    address_deserializer: AddressDeserializer,
    key_type_deserializer: KeyTypeDeserializer,
}

impl KeyDeserializer {
    /// Creates a new `KeyDeserializer`
    pub fn new(max_datastore_key_length: u8, datastore_key_length: bool) -> Self {
        Self {
            address_deserializer: AddressDeserializer::new(),
            key_type_deserializer: KeyTypeDeserializer::new(
                max_datastore_key_length,
                datastore_key_length,
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
        let (rest, key_type) = self.key_type_deserializer.deserialize(rest)?;

        Ok((rest, Key { address, key_type }))
    }
}
