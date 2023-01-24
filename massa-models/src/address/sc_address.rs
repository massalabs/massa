use crate::config::THREAD_COUNT;
use crate::error::ModelsError;
use crate::slot::{Slot, SlotDeserializer, SlotSerializer};
use massa_serialization::{
    DeserializeError, Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{context, ContextError, ParseError};
use nom::sequence::tuple;
use nom::{IResult, Parser};
use serde::{Deserialize, Serialize};
use std::ops::Bound::{Excluded, Included};

use super::AddressTrait;
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
#[allow(deprecated)]
///
pub struct SCAddress {
    slot: Slot,
    idx: u64,
    is_write: bool,
}

impl SCAddress {
    pub fn new(slot: Slot, idx: u64, is_write: bool) -> Self {
        Self {
            slot,
            idx,
            is_write,
        }
    }
    pub fn from_bytes(data: &[u8]) -> Result<SCAddress, ModelsError> {
        let deser = SCAddressDeserializer::new();
        let ([], sc_address): (&[u8], SCAddress) = deser.deserialize::<DeserializeError>(data).map_err(|er| ModelsError::DeserializeError(er.to_string()))? else {
            return Err( ModelsError::DeserializeError("leftover bytes".to_string()));
        };
        Ok(sc_address)
    }
}

impl AddressTrait for SCAddress {
    const PREFIX: u8 = b'S';
    const VERSION: u64 = 0;
    fn get_thread(&self, _thread_count: u8) -> u8 {
        self.slot.thread
    }

    fn to_bytes(&self) -> &[u8] {
        todo!()
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, ()>
    where
        Self: Sized,
    {
        todo!()
    }
}

/// Serializer for `Address`
#[derive(Default, Clone)]
pub struct SCAddressSerializer {
    version: U64VarIntSerializer,
    slot: SlotSerializer,
    idx: U64VarIntSerializer,
}

impl SCAddressSerializer {
    /// Serializes an `Address` into a `Vec<u8>`
    pub fn new() -> Self {
        Self {
            version: U64VarIntSerializer::new(),
            slot: SlotSerializer::new(),
            idx: U64VarIntSerializer::new(),
        }
    }
}

impl Serializer<SCAddress> for SCAddressSerializer {
    fn serialize(
        &self,
        value: &SCAddress,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        self.version.serialize(&SCAddress::VERSION, buffer)?;
        self.slot.serialize(&value.slot, buffer)?;
        self.idx.serialize(&value.idx, buffer)?;
        buffer.push(value.is_write as u8);
        Ok(())
    }
}
/// Deserializer for `SCAddress`
#[derive(Clone)]
pub struct SCAddressDeserializer {
    version: U64VarIntDeserializer,
    slot: SlotDeserializer,
    idx: U64VarIntDeserializer,
}

impl SCAddressDeserializer {
    /// Creates a new deserializer for `Address`
    pub const fn new() -> Self {
        Self {
            version: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(THREAD_COUNT)),
            ),
            idx: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<SCAddress> for SCAddressDeserializer {
    /// ## Example
    /// ```rust
    /// use massa_models::address::{SCAddress, Address, SCAddressDeserializer};
    /// use massa_models::slot::Slot;
    /// use massa_serialization::{Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let sc_address = SCAddress::new(Slot::new(0, 0), 0, false);
    /// let address = Address::SC(sc_address);
    /// let bytes = address.into_bytes();
    /// let (rest, res_addr) = SCAddressDeserializer::new().deserialize::<DeserializeError>(&bytes).unwrap();
    /// assert_eq!(address, res_addr.into());
    /// assert_eq!(rest.len(), 0);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], SCAddress, E> {
        context("SCAddress", |input| {
            {
                tuple((
                    context("Version", |input| self.version.deserialize(input)),
                    context("Slot", |input| self.slot.deserialize(input)),
                    context("index", |input| self.idx.deserialize(input)),
                ))
            }
            .parse(input)
        })
        .parse(buffer)
        .map(|(rest, (ver, slot, idx))| {
            dbg!(&ver);
            (
                rest,
                #[allow(deprecated)]
                SCAddress {
                    slot,
                    idx,
                    is_write: rest[0] == 1,
                },
            )
        })
    }
}
