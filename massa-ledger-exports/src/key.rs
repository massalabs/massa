use massa_hash::HashDeserializer;
use massa_models::{address::AddressDeserializer, Address};
use massa_serialization::{Deserializer, Serializer};

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
    fn serialize(&self, value: &Vec<u8>) -> Result<Vec<u8>, massa_serialization::SerializeError> {
        Ok(value.clone())
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
    fn deserialize<'a>(&self, buffer: &'a [u8]) -> nom::IResult<&'a [u8], Vec<u8>> {
        let (rest, address) = self.address_deserializer.deserialize(buffer)?;
        let error = nom::Err::Error(nom::error::Error::new(buffer, nom::error::ErrorKind::IsNot));
        match rest.first() {
            Some(ident) => match *ident {
                BALANCE_IDENT => Ok((&rest[1..], balance_key!(address))),
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

/// Extract an address from a key
pub fn get_address_from_key(key: &[u8]) -> Option<Address> {
    let address_deserializer = AddressDeserializer::new();
    address_deserializer.deserialize(key).map(|res| res.1).ok()
}
