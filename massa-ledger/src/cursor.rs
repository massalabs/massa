use massa_models::{
    array_from_slice, constants::ADDRESS_SIZE_BYTES, Address, DeserializeVarInt, Deserializer,
    ModelsError, SerializeVarInt, Serializer,
};

#[derive(Debug, Clone)]
pub enum LedgerCursorStep {
    Balance,
    Datastore(String),
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
            LedgerCursorStep::Balance => Ok(vec![0]),
            LedgerCursorStep::Datastore(key) => {
                let mut bytes = vec![1];
                bytes.extend((key.len() as u64).to_varint_bytes());
                bytes.extend_from_slice(key.as_bytes());
                Ok(bytes)
            }
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
            0 => Ok((LedgerCursorStep::Balance, cursor + 1)),
            1 => {
                cursor += 1;
                let (key_len, delta) = u64::from_varint_bytes(&bytes[cursor..])?;
                cursor += delta;
                let key = &bytes[cursor..cursor + key_len as usize];
                cursor += key_len as usize;
                Ok((
                    LedgerCursorStep::Datastore(String::from_utf8(key.to_vec()).unwrap()),
                    cursor,
                ))
            }
            _ => Err(ModelsError::DeserializeError(
                "Unknown cursor step".to_string(),
            )),
        }
    }
}

pub type LedgerCursor = (Address, LedgerCursorStep);

pub struct LedgerCursorSerializer {
    pub bootstrap_cursor_step_serializer: LedgerCursorStepSerializer,
}

impl LedgerCursorSerializer {
    pub fn new() -> Self {
        Self {
            bootstrap_cursor_step_serializer: LedgerCursorStepSerializer::new(),
        }
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

pub struct LedgerCursorDeserializer {
    pub bootstrap_cursor_step_deserializer: LedgerCursorStepDeserializer,
}

impl LedgerCursorDeserializer {
    pub fn new() -> Self {
        Self {
            bootstrap_cursor_step_deserializer: LedgerCursorStepDeserializer::new(),
        }
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
        Ok(((address, step), cursor))
    }
}
