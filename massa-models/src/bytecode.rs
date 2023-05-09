use crate::serialization::{VecU8Deserializer, VecU8Serializer};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{ContextError, ParseError};
use nom::IResult;
use serde::{Deserialize, Serialize};
use std::ops::Bound::Included;

/// Current version of the bytecode
pub(crate) const BYTECODE_VERSION: u64 = 0;

/// Structure representing executable bytecode
#[derive(Default, Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Bytecode(pub Vec<u8>);

/// Serializer for `Bytecode`
#[derive(Default)]
pub struct BytecodeSerializer {
    version_byte_serializer: U64VarIntSerializer,
    vec_u8_serializer: VecU8Serializer,
}

impl BytecodeSerializer {
    /// Creates a new `BytecodeSerializer`
    pub fn new() -> Self {
        Self {
            version_byte_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<Bytecode> for BytecodeSerializer {
    fn serialize(&self, value: &Bytecode, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.version_byte_serializer
            .serialize(&BYTECODE_VERSION, buffer)?;
        self.vec_u8_serializer.serialize(&value.0, buffer)?;
        Ok(())
    }
}

/// Deserializer for `Bytecode`
pub struct BytecodeDeserializer {
    version_byte_deserializer: U64VarIntDeserializer,
    vec_u8_deserializer: VecU8Deserializer,
}

impl BytecodeDeserializer {
    /// Creates a new `LedgerEntryDeserializer`
    pub fn new(max_datastore_value_length: u64) -> Self {
        Self {
            version_byte_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            vec_u8_deserializer: VecU8Deserializer::new(
                Included(u64::MIN),
                Included(max_datastore_value_length),
            ),
        }
    }
}

impl Deserializer<Bytecode> for BytecodeDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Bytecode, E> {
        let (rest, _version) = self.version_byte_deserializer.deserialize(buffer)?;
        let (rest, bytecode_vec) = self.vec_u8_deserializer.deserialize(rest)?;

        Ok((rest, Bytecode(bytecode_vec)))
    }
}
