use massa_hash::{Hash, HashDeserializer};
use massa_models::{address::AddressDeserializer, Address};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::IResult;

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
    fn _new() -> Self {
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
pub struct LedgerCursorStepDeserializer;

impl LedgerCursorStepDeserializer {
    fn _new() -> Self {
        Self
    }
}

impl Deserializer<LedgerCursorStep> for LedgerCursorStepDeserializer {
    fn deserialize<'a>(&self, bytes: &'a [u8]) -> IResult<&'a [u8], LedgerCursorStep> {
        match bytes[0] {
            0 => Ok((&bytes[1..], LedgerCursorStep::Start)),
            1 => Ok((&bytes[1..], LedgerCursorStep::Balance)),
            2 => Ok((&bytes[1..], LedgerCursorStep::Bytecode)),
            3 => {
                let hash_deserializer = HashDeserializer::new();
                if bytes[1..].is_empty() {
                    Ok((&bytes[1..], LedgerCursorStep::Datastore(None)))
                } else {
                    let (rest, key) = hash_deserializer.deserialize(&bytes[1..])?;
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
    bootstrap_cursor_step_serializer: LedgerCursorStepSerializer,
}

impl LedgerCursorSerializer {
    /// Creates a new ledger cursor serializer.
    pub fn _new() -> Self {
        Self {
            bootstrap_cursor_step_serializer: LedgerCursorStepSerializer::_new(),
        }
    }
}

impl Serializer<LedgerCursor> for LedgerCursorSerializer {
    fn serialize(&self, value: &LedgerCursor) -> Result<Vec<u8>, SerializeError> {
        let mut bytes = vec![];
        bytes.extend(value.0.to_bytes());
        bytes.extend(self.bootstrap_cursor_step_serializer.serialize(&value.1)?);
        Ok(bytes)
    }
}

/// A deserializer for the ledger cursor.
#[derive(Default)]
pub struct LedgerCursorDeserializer {
    bootstrap_cursor_step_deserializer: LedgerCursorStepDeserializer,
}

impl LedgerCursorDeserializer {
    /// Creates a new ledger cursor deserializer.
    pub fn _new() -> Self {
        Self {
            bootstrap_cursor_step_deserializer: LedgerCursorStepDeserializer::_new(),
        }
    }
}

impl Deserializer<LedgerCursor> for LedgerCursorDeserializer {
    fn deserialize<'a>(&self, bytes: &'a [u8]) -> IResult<&'a [u8], LedgerCursor> {
        let address_deserializer = AddressDeserializer::new();
        let (rest, address) = address_deserializer.deserialize(bytes)?;
        let (rest, step) = self.bootstrap_cursor_step_deserializer.deserialize(rest)?;
        Ok((rest, LedgerCursor(address, step)))
    }
}
