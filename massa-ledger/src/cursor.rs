use massa_hash::{Hash, HASH_SIZE_BYTES};
use massa_models::{
    array_from_slice, constants::ADDRESS_SIZE_BYTES, Address, Deserializer, ModelsError, Serializer,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerCursorStep {
    Start,
    Balance,
    Bytecode,
    StartDatastore,
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
            LedgerCursorStep::StartDatastore => Ok(vec![3]),
            LedgerCursorStep::Datastore(key) => {
                let mut bytes = vec![4];
                bytes.extend(key.to_bytes());
                Ok(bytes)
            }
            LedgerCursorStep::Finish => Ok(vec![5]),
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
    fn deserialize(&self, bytes: &[u8]) -> Result<(LedgerCursorStep, usize), ModelsError> {
        let mut cursor = 0;
        match bytes[cursor] {
            0 => Ok((LedgerCursorStep::Start, cursor + 1)),
            1 => Ok((LedgerCursorStep::Balance, cursor + 1)),
            2 => Ok((LedgerCursorStep::Bytecode, cursor + 1)),
            3 => Ok((LedgerCursorStep::StartDatastore, cursor + 1)),
            4 => {
                cursor += 1;
                let key = Hash::from_bytes(&array_from_slice(&bytes[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                Ok((LedgerCursorStep::Datastore(key), cursor))
            }
            5 => Ok((LedgerCursorStep::Finish, cursor + 1)),
            _ => Err(ModelsError::DeserializeError(
                "Unknown cursor step".to_string(),
            )),
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
    fn deserialize(&self, bytes: &[u8]) -> Result<(LedgerCursor, usize), ModelsError> {
        let mut cursor = 0;
        let address = Address::from_bytes(&array_from_slice(&bytes[cursor..])?)?;
        cursor += ADDRESS_SIZE_BYTES;
        let (step, delta) = self
            .bootstrap_cursor_step_deserializer
            .deserialize(&bytes[cursor..])?;
        cursor += delta;
        Ok((LedgerCursor(address, step), cursor))
    }
}
