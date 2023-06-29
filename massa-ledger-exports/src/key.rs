use massa_db_exports::LEDGER_PREFIX;
use massa_models::{
    address::{Address, AddressDeserializer, AddressSerializer},
    serialization::{VecU8Deserializer, VecU8Serializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{ContextError, ParseError};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

pub const VERSION_IDENT: u8 = 0u8;
pub const BALANCE_IDENT: u8 = 1u8;
pub const BYTECODE_IDENT: u8 = 2u8;
pub const DATASTORE_IDENT: u8 = 3u8;
pub const KEY_VERSION: u64 = 0;

#[derive(PartialEq, Eq, Clone, IntoPrimitive, TryFromPrimitive, Debug)]
#[repr(u8)]
enum KeyTypeId {
    Version = 0,
    Balance = 1,
    Bytecode = 2,
    Datastore = 3,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum KeyType {
    VERSION,
    BALANCE,
    BYTECODE,
    DATASTORE(Vec<u8>),
}

#[derive(Default, Clone)]
pub struct KeyTypeSerializer {
    vec_u8_serializer: VecU8Serializer,
    // Whether is deserialized with VecU8Deserializer or not.
    // If true, we use the VecU8Serializer to serialize the key which will add the length at the beginning.
    // If false, we just serialize the key as is.
    // This allows us to store the datastore key length at the beginning of the key or not.
    // The datastore key length is useful when transferring multiple keys, like in packets,
    // but isn't when storing a datastore key in the ledger.
    with_datastore_key_length: bool,
}

impl KeyTypeSerializer {
    /// Creates a new KeyTypeSerializer.
    /// `with_datastore_key_length` if true, the datastore key is serialized with its length.
    pub fn new(with_datastore_key_length: bool) -> Self {
        Self {
            vec_u8_serializer: VecU8Serializer::new(),
            with_datastore_key_length,
        }
    }
}

impl Serializer<KeyType> for KeyTypeSerializer {
    fn serialize(&self, value: &KeyType, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            KeyType::VERSION => buffer.extend(&[u8::from(KeyTypeId::Version)]),
            KeyType::BALANCE => buffer.extend(&[u8::from(KeyTypeId::Balance)]),
            KeyType::BYTECODE => buffer.extend(&[u8::from(KeyTypeId::Bytecode)]),
            KeyType::DATASTORE(data) => {
                buffer.extend(&[u8::from(KeyTypeId::Datastore)]);
                if self.with_datastore_key_length {
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
    // Same as in KeyTypeSerializer but for deserialization.
    with_datastore_key_length: bool,
}

impl KeyTypeDeserializer {
    /// Creates a new KeyTypeDeserializer.
    /// `max_datastore_key_length` is the maximum length of a datastore key.
    /// `with_datastore_key_length` if true, the datastore key is deserialized with its length.
    pub fn new(max_datastore_key_length: u8, with_datastore_key_length: bool) -> Self {
        Self {
            vec_u8_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_key_length as u64),
            ),
            with_datastore_key_length,
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
                if self.with_datastore_key_length {
                    let (rest, data) = self.vec_u8_deserializer.deserialize(rest)?;
                    Ok((rest, KeyType::DATASTORE(data)))
                } else {
                    Ok((&[], KeyType::DATASTORE(rest.to_vec())))
                }
            }
            Ok(KeyTypeId::Version) => Ok((rest, KeyType::VERSION)),
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

/// Gives the general prefix of the datastore of an address while respecting a provided key prefix
pub fn datastore_prefix_from_address(address: &Address, prefix: &[u8]) -> Vec<u8> {
    let mut res_prefix = LEDGER_PREFIX.as_bytes().to_vec();
    U64VarIntSerializer::new()
        .serialize(&KEY_VERSION, &mut res_prefix)
        .unwrap();
    AddressSerializer::new()
        .serialize(address, &mut res_prefix)
        .unwrap();
    res_prefix.push(DATASTORE_IDENT);
    res_prefix.extend(prefix);
    res_prefix
}

/// Basic key serializer
#[derive(Default, Clone)]
pub struct KeySerializer {
    address_serializer: AddressSerializer,
    key_type_serializer: KeyTypeSerializer,
    version_byte_serializer: U64VarIntSerializer,
}

impl KeySerializer {
    /// Creates a new `KeySerializer`
    /// `with_datastore_key_length` if true, the datastore key is serialized with its length.
    pub fn new(with_datastore_key_length: bool) -> Self {
        Self {
            address_serializer: AddressSerializer::new(),
            key_type_serializer: KeyTypeSerializer::new(with_datastore_key_length),
            version_byte_serializer: U64VarIntSerializer::new(),
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
    /// KeySerializer::new(true).serialize(&key, &mut serialized).unwrap();
    /// ```
    fn serialize(&self, value: &Key, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        buffer.extend(LEDGER_PREFIX.as_bytes());

        self.version_byte_serializer
            .serialize(&KEY_VERSION, buffer)?;
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
    version_byte_deserializer: U64VarIntDeserializer,
}

impl KeyDeserializer {
    /// Creates a new `KeyDeserializer`
    /// `max_datastore_key_length` is the maximum length of a datastore key.
    /// `with_datastore_key_length` if true, the datastore key is deserialized with its length.
    pub fn new(max_datastore_key_length: u8, with_datastore_key_length: bool) -> Self {
        Self {
            address_deserializer: AddressDeserializer::new(),
            key_type_deserializer: KeyTypeDeserializer::new(
                max_datastore_key_length,
                with_datastore_key_length,
            ),
            version_byte_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
        }
    }
}

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
    /// KeySerializer::new(true).serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new(255, true).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(key_deser, key);
    ///
    /// let mut key = Key::new(&address, KeyType::BALANCE);
    /// let mut serialized = Vec::new();
    /// KeySerializer::new(true).serialize(&key, &mut serialized).unwrap();
    /// let (rest, key_deser) = KeyDeserializer::new(255, true).deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(key_deser, key);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> nom::IResult<&'a [u8], Key, E> {
        let (rest, _version) = self
            .version_byte_deserializer
            .deserialize(&buffer[LEDGER_PREFIX.as_bytes().len()..])?;
        let (rest, address) = self.address_deserializer.deserialize(rest)?;
        let (rest, key_type) = self.key_type_deserializer.deserialize(rest)?;

        Ok((rest, Key { address, key_type }))
    }
}
