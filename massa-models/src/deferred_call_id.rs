use std::{ops::Bound, str::FromStr};

use massa_serialization::{
    DeserializeError, Deserializer, SerializeError, Serializer, U64VarIntDeserializer,
    U64VarIntSerializer,
};
use nom::{
    error::{ContextError, ParseError},
    IResult,
};
use transition::Versioned;

// use std::collections::Bound;

use crate::{
    config::THREAD_COUNT,
    error::ModelsError,
    serialization::{VecU8Deserializer, VecU8Serializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
};

const DEFERRED_CALL_ID_PREFIX: &str = "D";

/// block id
#[allow(missing_docs)]
#[transition::versioned(versions("0"))]
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct DeferredCallId(Vec<u8>);

/// Serializer for `DeferredCallId`
#[derive(Default, Clone)]
pub struct DeferredCallIdSerializer {
    bytes_serializer: VecU8Serializer,
}

impl DeferredCallIdSerializer {
    /// Serializes an `DeferredCallId` into a `Vec<u8>`
    pub fn new() -> Self {
        Self {
            bytes_serializer: VecU8Serializer::new(),
        }
    }
}

impl Serializer<DeferredCallId> for DeferredCallIdSerializer {
    fn serialize(
        &self,
        value: &DeferredCallId,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            DeferredCallId::DeferredCallIdV0(id) => {
                self.bytes_serializer.serialize(&id.0, buffer)?;
            }
        }
        Ok(())
    }
}

/// Deserializer for `DeferredCallId`
#[derive(Clone)]
pub struct DeferredCallIdDeserializer {
    bytes_deserializer: VecU8Deserializer,
}

impl DeferredCallIdDeserializer {
    /// Deserializes a `Vec<u8>` into an `DeferredCallId`
    pub fn new() -> Self {
        Self {
            bytes_deserializer: VecU8Deserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(u64::MAX),
            ),
        }
    }
}

impl Deserializer<DeferredCallId> for DeferredCallIdDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], DeferredCallId, E> {
        let (rest, bytes) = self.bytes_deserializer.deserialize(buffer)?;
        Ok((
            rest,
            DeferredCallId::DeferredCallIdV0(DeferredCallIdV0(bytes)),
        ))
    }
}

impl FromStr for DeferredCallId {
    type Err = ModelsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with(DEFERRED_CALL_ID_PREFIX) {
            return Err(ModelsError::DeserializeError(format!(
                "Invalid prefix for DeferredCallId: {}",
                s
            )));
        }
        let s = &s[DEFERRED_CALL_ID_PREFIX.len()..];
        let bytes = bs58::decode(s).with_check(None).into_vec().map_err(|_| {
            ModelsError::DeserializeError(format!(
                "Invalid base58 string for DeferredCallId: {}",
                s
            ))
        })?;
        DeferredCallId::from_bytes(&bytes)
    }
}

impl std::fmt::Display for DeferredCallId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            DEFERRED_CALL_ID_PREFIX,
            bs58::encode(self.as_bytes()).with_check().into_string()
        )
    }
}

impl ::serde::Serialize for DeferredCallId {
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

impl<'de> ::serde::Deserialize<'de> for DeferredCallId {
    fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> Result<DeferredCallId, D::Error> {
        if d.is_human_readable() {
            struct DeferredCallIdVisitor;

            impl<'de> ::serde::de::Visitor<'de> for DeferredCallIdVisitor {
                type Value = DeferredCallId;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("ASC + base58::encode(bytes)")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    if let Ok(v_str) = std::str::from_utf8(v) {
                        DeferredCallId::from_str(v_str).map_err(E::custom)
                    } else {
                        Err(E::invalid_value(::serde::de::Unexpected::Bytes(v), &self))
                    }
                }

                fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    DeferredCallId::from_str(v).map_err(E::custom)
                }
            }
            d.deserialize_str(DeferredCallIdVisitor)
        } else {
            struct BytesVisitor;

            impl<'de> ::serde::de::Visitor<'de> for BytesVisitor {
                type Value = DeferredCallId;

                fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                    formatter.write_str("[u64varint-of-addr-variant][u64varint-of-version][bytes]")
                }

                fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
                where
                    E: ::serde::de::Error,
                {
                    DeferredCallId::from_bytes(v).map_err(E::custom)
                }
            }

            d.deserialize_bytes(BytesVisitor)
        }
    }
}

impl DeferredCallId {
    /// Return the slot of the `DeferredCallId`
    pub fn get_slot(&self) -> Result<Slot, ModelsError> {
        let version_deserializer = U64VarIntDeserializer::new(
            std::ops::Bound::Included(0),
            std::ops::Bound::Included(u64::MAX),
        );

        let slot_deser = SlotDeserializer::new(
            (Bound::Included(0), Bound::Included(u64::MAX)),
            (Bound::Included(0), Bound::Excluded(THREAD_COUNT)),
        );

        let (rest, _version) = version_deserializer
            .deserialize::<DeserializeError>(self.as_bytes())
            .map_err(|_e| ModelsError::DeferredCallIdParseError)?;
        let (_rest, slot) = slot_deser
            .deserialize::<DeserializeError>(rest)
            .map_err(|_e| ModelsError::DeferredCallIdParseError)?;
        Ok(slot)
    }

    /// Create a new `DeferredCallId`
    pub fn new(
        version: u64,
        target_slot: Slot,
        index: u64,
        trail_hash: &[u8],
    ) -> Result<Self, ModelsError> {
        let mut id: Vec<u8> = Vec::new();
        match version {
            0 => {
                let version_serializer = U64VarIntSerializer::new();
                let slot_serializer = SlotSerializer::new();
                version_serializer.serialize(&version, &mut id)?;
                slot_serializer.serialize(&target_slot, &mut id)?;
                id.extend(index.to_be_bytes());
                id.extend(trail_hash);
                Ok(DeferredCallId::DeferredCallIdV0(DeferredCallIdV0(id)))
            }
            _ => {
                return Err(ModelsError::InvalidVersionError(format!(
                    "Invalid version to create an DeferredCallId: {}",
                    version
                )))
            }
        }
    }

    /// Return the version of the `DeferredCallId` as bytes
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            DeferredCallId::DeferredCallIdV0(block_id) => block_id.as_bytes(),
        }
    }

    /// Create an `DeferredCallId` from bytes
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
                let id = DeferredCallIdV0::from_bytes(bytes)?;
                Ok(DeferredCallId::DeferredCallIdV0(id))
            }
            _ => Err(ModelsError::InvalidVersionError(format!(
                "Invalid version to create an DeferredCallId: {}",
                version
            ))),
        }
    }
}

#[transition::impl_version(versions("0"))]
impl DeferredCallId {
    /// Return the version of the `DeferredCallId` as bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Create an `DeferredCallId` from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ModelsError> {
        Ok(DeferredCallId(bytes.to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use massa_serialization::DeserializeError;

    use super::*;
    use crate::slot::Slot;

    #[test]
    fn test_deferred_call_id_ser_deser() {
        let slot = Slot::new(1, 2);
        let index = 3;
        let trail_hash = [4, 5, 6];
        let id = DeferredCallId::new(0, slot, index, &trail_hash).unwrap();
        let serializer = DeferredCallIdSerializer::new();
        let mut buffer = Vec::new();
        serializer.serialize(&id, &mut buffer).unwrap();
        let deserializer = DeferredCallIdDeserializer::new();
        let (rest, deserialized_id) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert_eq!(deserialized_id, id);
        assert!(rest.is_empty());
    }

    #[test]
    fn test_deferred_call_id_from_str() {
        let slot = Slot::new(1, 2);
        let index = 3;
        let trail_hash = [4, 5, 6];
        let id = DeferredCallId::new(0, slot, index, &trail_hash).unwrap();
        let id_str = id.to_string();
        let deserialized_id = DeferredCallId::from_str(&id_str).unwrap();
        assert_eq!(deserialized_id, id);
    }

    #[test]
    fn test_get_slot() {
        let slot = Slot::new(1, 2);
        let index = 3;
        let trail_hash = [4, 5, 6];
        let id = DeferredCallId::new(0, slot, index, &trail_hash).unwrap();
        assert_eq!(id.get_slot().unwrap(), slot);
    }
}
