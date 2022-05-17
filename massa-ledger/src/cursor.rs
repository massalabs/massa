use massa_hash::Hash;
use massa_models::{Address, Deserializer, ModelsError, Serializer};
use nom::IResult;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerCursorStep {
    Start,
    Balance,
    Bytecode,
    Datastore(Hash),
    Finish,
}

#[derive(Debug, Clone)]
pub struct LedgerCursorStepSerializer;

impl LedgerCursorStepSerializer {
    fn new() -> Self {
        Self
    }
}

impl Serializer<LedgerCursorStep> for LedgerCursorStepSerializer {
    fn serialize(&self, value: &LedgerCursorStep) -> Result<Vec<u8>, ModelsError> {
        match value {
            LedgerCursorStep::Start => Ok(vec![0]),
            LedgerCursorStep::Balance => Ok(vec![1]),
            LedgerCursorStep::Bytecode => Ok(vec![2]),
            LedgerCursorStep::Datastore(key) => {
                let mut bytes = vec![3];
                bytes.extend(key.to_bytes());
                Ok(bytes)
            }
            LedgerCursorStep::Finish => Ok(vec![4]),
        }
    }
}

pub struct LedgerCursorStepDeserializer;

impl LedgerCursorStepDeserializer {
    fn new() -> Self {
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
                let (rest, key) = Hash::nom_deserialize(&bytes[1..])?;
                Ok((rest, LedgerCursorStep::Datastore(key)))
            }
            4 => Ok((&bytes[1..], LedgerCursorStep::Finish)),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                bytes,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
/// A cursor to iterate through the ledger with different granularity.
pub struct LedgerCursor(pub Address, pub LedgerCursorStep);

/// A serializer for the ledger cursor.
pub struct LedgerCursorSerializer {
    bootstrap_cursor_step_serializer: LedgerCursorStepSerializer,
}

impl LedgerCursorSerializer {
    /// Creates a new ledger cursor serializer.
    pub fn new() -> Self {
        Self {
            bootstrap_cursor_step_serializer: LedgerCursorStepSerializer::new(),
        }
    }
}

impl Default for LedgerCursorSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<LedgerCursor> for LedgerCursorSerializer {
    fn serialize(&self, value: &LedgerCursor) -> Result<Vec<u8>, ModelsError> {
        let mut bytes = vec![];
        bytes.extend(value.0.to_bytes());
        bytes.extend(self.bootstrap_cursor_step_serializer.serialize(&value.1)?);
        Ok(bytes)
    }
}

/// A deserializer for the ledger cursor.
pub struct LedgerCursorDeserializer {
    bootstrap_cursor_step_deserializer: LedgerCursorStepDeserializer,
}

impl LedgerCursorDeserializer {
    /// Creates a new ledger cursor deserializer.
    pub fn new() -> Self {
        Self {
            bootstrap_cursor_step_deserializer: LedgerCursorStepDeserializer::new(),
        }
    }
}

impl Default for LedgerCursorDeserializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Deserializer<LedgerCursor> for LedgerCursorDeserializer {
    fn deserialize<'a>(&self, bytes: &'a [u8]) -> IResult<&'a [u8], LedgerCursor> {
        let (rest, address) = Address::nom_deserialize(bytes)?;
        let (rest, step) = self.bootstrap_cursor_step_deserializer.deserialize(&rest)?;
        Ok((rest, LedgerCursor(address, step)))
    }
}
