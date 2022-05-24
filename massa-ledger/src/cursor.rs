use massa_hash::{Hash, HashDeserializer};
use massa_models::{address::AddressDeserializer, Address};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{sequence::tuple, IResult};

/// When sending the ledger we need to split the messages so that we don't send the whole ledger or a huge part of the datastore in one message.
/// We have 5 different state where the cursor can stop that define 5 different positions in the ledger:
/// `Start`: At the start of a new address
/// `Balance`: Before the balance of an address
/// `Bytecode`: Before the bytecode of an address
/// `Datastore`: If `Hash` is None then it's before the datastore otherwise it's after the key represented by the `Hash``
/// `Finish`: After the encoding of the whole data of an address
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
pub struct LedgerCursorStepDeserializer {
    hash_deserializer: HashDeserializer,
}

impl LedgerCursorStepDeserializer {
    fn _new() -> Self {
        Self {
            hash_deserializer: HashDeserializer::new(),
        }
    }
}

impl Deserializer<LedgerCursorStep> for LedgerCursorStepDeserializer {
    fn deserialize<'a>(&self, bytes: &'a [u8]) -> IResult<&'a [u8], LedgerCursorStep> {
        match bytes.get(0) {
            Some(0) => Ok((&bytes[1..], LedgerCursorStep::Start)),
            Some(1) => Ok((&bytes[1..], LedgerCursorStep::Balance)),
            Some(2) => Ok((&bytes[1..], LedgerCursorStep::Bytecode)),
            Some(3) => {
                if bytes[1..].is_empty() {
                    Ok((&bytes[1..], LedgerCursorStep::Datastore(None)))
                } else {
                    let (rest, key) = self.hash_deserializer.deserialize(&bytes[1..])?;
                    Ok((rest, LedgerCursorStep::Datastore(Some(key))))
                }
            }
            Some(4) => Ok((&bytes[1..], LedgerCursorStep::Finish)),
            _ => Err(nom::Err::Error(nom::error::Error::new(
                bytes,
                nom::error::ErrorKind::Digit,
            ))),
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
/// A cursor to iterate through the ledger with different granularity.
pub struct LedgerCursor {
    pub address: Address,
    pub step: LedgerCursorStep,
}

/// A serializer for the ledger cursor.
#[derive(Default)]
pub struct LedgerCursorSerializer {
    cursor_step_serializer: LedgerCursorStepSerializer,
}

impl LedgerCursorSerializer {
    /// Creates a new ledger cursor serializer.
    pub fn _new() -> Self {
        Self {
            cursor_step_serializer: LedgerCursorStepSerializer::_new(),
        }
    }
}

impl Serializer<LedgerCursor> for LedgerCursorSerializer {
    fn serialize(&self, value: &LedgerCursor) -> Result<Vec<u8>, SerializeError> {
        let mut bytes = vec![];
        bytes.extend(value.address.to_bytes());
        bytes.extend(self.cursor_step_serializer.serialize(&value.step)?);
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
    pub fn _new() -> Self {
        Self {
            cursor_step_deserializer: LedgerCursorStepDeserializer::_new(),
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
        let (rest, (address, step)) = parser(input)?;
        Ok((rest, LedgerCursor { address, step }))
    }
}
