use massa_hash::{Hash, HashDeserializer};
use massa_models::{address::AddressDeserializer, Address};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{sequence::tuple, IResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerCursorStep {
    Start,
    Balance,
    Bytecode,
    Datastore(Option<Hash>),
    Finish,
}

#[derive(Debug, Clone, Default)]
pub struct LedgerCursorStepSerializer;

impl LedgerCursorStepSerializer {
    fn new() -> Self {
        Self
    }
}

impl Serializer<LedgerCursorStep> for LedgerCursorStepSerializer {
    fn serialize(&self, value: &LedgerCursorStep) -> Result<Vec<u8>, SerializeError> {
        match value {
            LedgerCursorStep::Start => Ok(vec![0]),
            LedgerCursorStep::Balance => Ok(vec![1]),
            LedgerCursorStep::Bytecode => Ok(vec![2]),
            LedgerCursorStep::Datastore(key) => {
                let mut bytes = vec![3];
                if let Some(key) = key {
                    bytes.extend(key.to_bytes());
                }
                Ok(bytes)
            }
            LedgerCursorStep::Finish => Ok(vec![4]),
        }
    }
}

#[derive(Default)]
pub struct LedgerCursorStepDeserializer {
    hash_deserializer: HashDeserializer,
}

impl LedgerCursorStepDeserializer {
    fn new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<LedgerCursorStep> for LedgerCursorStepDeserializer {
    fn deserialize<'a>(&self, bytes: &'a [u8]) -> IResult<&'a [u8], LedgerCursorStep> {
        match bytes[0] {
            0 => Ok((&bytes[1..], LedgerCursorStep::Start)),
            1 => Ok((&bytes[1..], LedgerCursorStep::Balance)),
            2 => Ok((&bytes[1..], LedgerCursorStep::Bytecode)),
            3 => {
                if bytes[1..].is_empty() {
                    Ok((&bytes[1..], LedgerCursorStep::Datastore(None)))
                } else {
                    let (rest, key) = self.hash_deserializer.deserialize(&bytes[1..])?;
                    Ok((rest, LedgerCursorStep::Datastore(Some(key))))
                }
            }
            4 => Ok((&bytes[1..], LedgerCursorStep::Finish)),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                bytes,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
/// A cursor to iterate through the ledger with different granularity.
pub struct LedgerCursor(pub Address, pub LedgerCursorStep);

/// A serializer for the ledger cursor.
#[derive(Default)]
pub struct LedgerCursorSerializer {
    cursor_step_serializer: LedgerCursorStepSerializer,
}

impl LedgerCursorSerializer {
    /// Creates a new ledger cursor serializer.
    pub fn new() -> Self {
        Self {
            cursor_step_serializer: LedgerCursorStepSerializer::new(),
        }
    }
}

impl Serializer<LedgerCursor> for LedgerCursorSerializer {
    fn serialize(&self, value: &LedgerCursor) -> Result<Vec<u8>, SerializeError> {
        let mut bytes = vec![];
        bytes.extend(value.0.to_bytes());
        bytes.extend(self.cursor_step_serializer.serialize(&value.1)?);
        Ok(bytes)
    }
}

/// A deserializer for the ledger cursor.
#[derive(Default)]
pub struct LedgerCursorDeserializer {
    cursor_step_deserializer: LedgerCursorStepDeserializer,
    address_deserializer: AddressDeserializer,
}

impl LedgerCursorDeserializer {
    /// Creates a new ledger cursor deserializer.
    pub fn new() -> Self {
        Self {
            cursor_step_deserializer: LedgerCursorStepDeserializer::new(),
            address_deserializer: AddressDeserializer::new(),
        }
    }
}

impl Deserializer<LedgerCursor> for LedgerCursorDeserializer {
    fn deserialize<'a>(&self, input: &'a [u8]) -> IResult<&'a [u8], LedgerCursor> {
        let mut parser = tuple((
            |input| self.address_deserializer.deserialize(input),
            |input| self.cursor_step_deserializer.deserialize(input),
        ));
        let (rest, (address, cursor)) = parser(input)?;
        Ok((rest, LedgerCursor(address, cursor)))
    }
}
