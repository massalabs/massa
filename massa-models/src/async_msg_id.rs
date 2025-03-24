#![allow(missing_docs)]

use massa_serialization::{Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use num::rational::Ratio;
use std::ops::Bound::{Excluded, Included};

use crate::slot::{Slot, SlotDeserializer, SlotSerializer};

/// Unique identifier of a message.
/// Also has the property of ordering by priority (highest first) following the triplet:
/// `(rev(Ratio(msg.fee, max(msg.max_gas,1))), emission_slot, emission_index)`
pub type AsyncMessageId = (std::cmp::Reverse<Ratio<u64>>, Slot, u64);

#[derive(Clone)]
pub struct AsyncMessageIdSerializer {
    slot_serializer: SlotSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl AsyncMessageIdSerializer {
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
        }
    }
}

impl Default for AsyncMessageIdSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<AsyncMessageId> for AsyncMessageIdSerializer {
    /// ## Example
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::Serializer;
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use std::str::FromStr;
    /// use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncMessageIdSerializer};
    ///
    /// let message = AsyncMessage::new(
    ///     Slot::new(1, 0),
    ///     0,
    ///     Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     String::from("test"),
    ///     10000000,
    ///     Amount::from_str("1").unwrap(),
    ///     Amount::from_str("1").unwrap(),
    ///     Slot::new(2, 0),
    ///     Slot::new(3, 0),
    ///     vec![1, 2, 3, 4],
    ///     None,
    ///     None
    /// );
    /// let id: AsyncMessageId = message.compute_id();
    /// let mut serialized = Vec::new();
    /// let serializer = AsyncMessageIdSerializer::new();
    /// serializer.serialize(&id, &mut serialized).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &AsyncMessageId,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        self.u64_serializer.serialize(value.0 .0.numer(), buffer)?;
        self.u64_serializer.serialize(value.0 .0.denom(), buffer)?;
        self.slot_serializer.serialize(&value.1, buffer)?;
        self.u64_serializer.serialize(&value.2, buffer)?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct AsyncMessageIdDeserializer {
    slot_deserializer: SlotDeserializer,
    numerator_deserializer: U64VarIntDeserializer,
    denominator_deserializer: U64VarIntDeserializer,
    emission_index_deserializer: U64VarIntDeserializer,
}

impl AsyncMessageIdDeserializer {
    pub fn new(thread_count: u8) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            numerator_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(u64::MAX),
            ),
            denominator_deserializer: U64VarIntDeserializer::new(Included(1), Included(u64::MAX)),
            emission_index_deserializer: U64VarIntDeserializer::new(
                Included(u64::MIN),
                Included(u64::MAX),
            ),
        }
    }
}

impl Deserializer<AsyncMessageId> for AsyncMessageIdDeserializer {
    /// ## Example
    /// ```
    /// use std::ops::Bound::Included;
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use std::str::FromStr;
    /// use massa_async_pool::{AsyncMessage, AsyncMessageId, AsyncMessageIdSerializer, AsyncMessageIdDeserializer};
    ///
    /// let message = AsyncMessage::new(
    ///     Slot::new(1, 0),
    ///     0,
    ///     Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///     Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
    ///     String::from("test"),
    ///     10000000,
    ///     Amount::from_str("1").unwrap(),
    ///     Amount::from_str("1").unwrap(),
    ///     Slot::new(2, 0),
    ///     Slot::new(3, 0),
    ///     vec![1, 2, 3, 4],
    ///     None,
    ///     None
    /// );
    /// let id: AsyncMessageId = message.compute_id();
    /// let mut serialized = Vec::new();
    /// let serializer = AsyncMessageIdSerializer::new();
    /// let deserializer = AsyncMessageIdDeserializer::new(10);
    /// serializer.serialize(&id, &mut serialized).unwrap();
    /// let (rest, id_deser) = deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(id, id_deser);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncMessageId, E> {
        context(
            "Failed AsyncMessageId deserialization",
            tuple((
                context("Failed num deserialization", |input| {
                    self.numerator_deserializer.deserialize(input)
                }),
                context("Failed denum deserialization", |input| {
                    self.denominator_deserializer.deserialize(input)
                }),
                context("Failed emission_slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed emission_index deserialization", |input| {
                    self.emission_index_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(fee, denom, slot, index)| (std::cmp::Reverse(Ratio::new(fee, denom)), slot, index))
        .parse(buffer)
    }
}
