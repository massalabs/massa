use std::str::FromStr;

use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{ContextError, ErrorKind, ParseError},
    IResult,
};
use transition::Versioned;

use crate::{
    error::ModelsError,
    serialization::{VecU8Deserializer, VecU8Serializer},
    slot::{Slot, SlotSerializer},
};

const ASC_CALL_ID_PREFIX: &str = "ASC";

/// block id
#[allow(missing_docs)]
#[transition::versioned(versions("0"))]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct AsyncCallId(Vec<u8>);

/// Serializer for `AsyncCallId`
#[derive(Default, Clone)]
pub struct AsyncCallIdSerializer {
    bytes_serializer: VecU8Serializer,
}

impl AsyncCallIdSerializer {
    /// Serializes an `AsyncCallId` into a `Vec<u8>`
    pub fn new() -> Self {
        Self {
            bytes_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<AsyncCallId> for AsyncCallIdSerializer {
    fn serialize(&self, value: &AsyncCallId, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match value {
            AsyncCallId::AsyncCallIdV0(id) => {
                self.bytes_serializer.serialize(&id.0, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `AsyncCallId`
pub struct AsyncCallIdDeserializer {
    bytes_deserializer: VecU8Deserializer,
}

impl AsyncCallIdDeserializer {
    /// Deserializes a `Vec<u8>` into an `AsyncCallId`
    pub fn new() -> Self {
        Self {
            bytes_deserializer: VecU8Deserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(u64::MAX),
            ),
        }
    }
}

impl Deserializer<AsyncCallId> for AsyncCallIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncCallId, E> {
        let (rest, bytes) = self.bytes_deserializer.deserialize(buffer)?;
        Ok((rest, AsyncCallId::AsyncCallIdV0(AsyncCallIdV0(bytes))))
    }
}

impl FromStr for AsyncCallId {
    type Err = ModelsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with(ASC_CALL_ID_PREFIX) {
            return Err(ModelsError::DeserializeError(format!(
                "Invalid prefix for AsyncCallId: {}",
                s
            )));
        }
        let s = &s[ASC_CALL_ID_PREFIX.len()..];
        let bytes = bs58::decode(s).with_check(None).into_vec().map_err(|_| {
            ModelsError::DeserializeError(format!("Invalid base58 string for AsyncCallId: {}", s))
        })?;
        AsyncCallId::from_bytes(&bytes)
    }
}

impl std::fmt::Display for AsyncCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            ASC_CALL_ID_PREFIX,
            bs58::encode(self.as_bytes()).with_check().into_string()
        )
    }
}

impl ::serde::Serialize for AsyncCallId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ::serde::Serializer,
    {
        if serializer.is_human_readable() {
            serializer.collect_str(&self.to_string())
        } else {
            serializer.serialize_bytes(&self.as_bytes())
        }
    }
}

impl<'de> ::serde::Deserialize<'de> for AsyncCallId {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<AsyncCallId, D::Error> {
        if d.is_human_readable() {
            struct AsyncCallIdVisitor;

            impl<'de> ::serde::de::Visitor<'de> for AsyncCallIdVisitor {
                type Value = AsyncCallId;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("ASC + base58::encode(bytes)")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        AsyncCallId::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    AsyncCallId::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(AsyncCallIdVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = AsyncCallId;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("[u64varint-of-addr-variant][u64varint-of-version][bytes]")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    AsyncCallId::from_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl AsyncCallId {
    /// Create a new `AsyncCallId`
    pub fn new(
        version: u64,
        target_slot: Slot,
        index: u64,
        trail_hash: &[u8],
    ) -> Result<Self, ModelsError> {
        let mut id = Vec::new();
        match version {
            0 => {
                let version_serializer = U64VarIntSerializer::new();
                let slot_serializer = SlotSerializer::new();
                version_serializer.serialize(&version, &mut id)?;
                slot_serializer.serialize(&target_slot, &mut id)?;
                id.extend(index.to_be_bytes());
                id.extend(trail_hash);
                Ok(AsyncCallId::AsyncCallIdV0(AsyncCallIdV0(id)))
            }
            _ => {
                return Err(ModelsError::InvalidVersionError(format!(
                    "Invalid version to create an AsyncCallId: {}",
                    version
                )))
            }
        }
    }

    /// Return the version of the `AsyncCallId` as bytes
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            AsyncCallId::AsyncCallIdV0(block_id) => block_id.as_bytes(),
        }
    }

    /// Create an `AsyncCallId` from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ModelsError> {
        if bytes.is_empty() {
            return Err(ModelsError::SerializeError("Empty bytes".to_string()));
        }
        let version = U64VarIntDeserializer::new(
            std::ops::Bound::Included(0),
            std::ops::Bound::Included(u64::MAX),
        );
        let (_, version) = version.deserialize(bytes)?;
        match version {
            0 => {
                let id = AsyncCallIdV0::from_bytes(bytes)?;
                Ok(AsyncCallId::AsyncCallIdV0(id))
            }
            _ => Err(ModelsError::InvalidVersionError(format!(
                "Invalid version to create an AsyncCallId: {}",
                version
            ))),
        }
    }
}

#[transition::impl_version(versions("0"))]
impl AsyncCallId {
    /// Return the version of the `AsyncCallId` as bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Create an `AsyncCallId` from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ModelsError> {
        Ok(AsyncCallIdV0(bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use massa_serialization::DeserializeError;

    use super::*;
    use crate::slot::Slot;

    #[test]
    fn test_async_call_id_ser_deser() {
        let slot = Slot::new(1, 2);
        let index = 3;
        let trail_hash = [4, 5, 6];
        let id = AsyncCallId::new(0, slot, index, &trail_hash).unwrap();
        let serializer = AsyncCallIdSerializer::new();
        let mut buffer = Vec::new();
        serializer.serialize(&id, &mut buffer).unwrap();
        let deserializer = AsyncCallIdDeserializer::new();
        let (rest, deserialized_id) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert_eq!(deserialized_id, id);
        assert!(rest.is_empty());
    }

    #[test]
    fn test_async_call_id_from_str() {
        let slot = Slot::new(1, 2);
        let index = 3;
        let trail_hash = [4, 5, 6];
        let id = AsyncCallId::new(0, slot, index, &trail_hash).unwrap();
        let id_str = id.to_string();
        let deserialized_id = AsyncCallId::from_str(&id_str).unwrap();
        assert_eq!(deserialized_id, id);
    }
}
