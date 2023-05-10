//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the structure representing an asynchronous message

use massa_hash::Hash;
use massa_models::address::{AddressDeserializer, AddressSerializer};
use massa_models::amount::{AmountDeserializer, AmountSerializer};
use massa_models::slot::{SlotDeserializer, SlotSerializer};
use massa_models::{
    address::Address,
    amount::Amount,
    serialization::{VecU8Deserializer, VecU8Serializer},
    slot::Slot,
};
use massa_serialization::{
    Deserializer, OptionDeserializer, OptionSerializer, SerializeError, Serializer,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::error::{context, ContextError, ParseError};
use nom::multi::length_data;
use nom::sequence::tuple;
use nom::{IResult, Parser};
use num::rational::Ratio;
use serde::{Deserialize, Serialize};
use std::ops::Bound::{Excluded, Included};

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
    /// let message = AsyncMessage::new_with_hash(
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
    u64_deserializer: U64VarIntDeserializer,
}

impl AsyncMessageIdDeserializer {
    pub fn new(thread_count: u8) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
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
    /// let message = AsyncMessage::new_with_hash(
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
                context("Failed fee deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed denum deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
                context("Failed emission_slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed emission_index deserialization", |input| {
                    self.u64_deserializer.deserialize(input)
                }),
            )),
        )
        .map(|(fee, denom, slot, index)| (std::cmp::Reverse(Ratio::new(fee, denom)), slot, index))
        .parse(buffer)
    }
}

/// Structure defining a trigger for an asynchronous message
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AsyncMessageTrigger {
    /// Filter on the address
    pub address: Address,

    /// Filter on the datastore key
    pub datastore_key: Option<Vec<u8>>,
}

/// Serializer for a trigger for an asynchronous message
struct AsyncMessageTriggerSerializer {
    address_serializer: AddressSerializer,
    key_serializer: OptionSerializer<Vec<u8>, VecU8Serializer>,
}

impl AsyncMessageTriggerSerializer {
    pub(crate) fn new() -> Self {
        Self {
            address_serializer: AddressSerializer::new(),
            key_serializer: OptionSerializer::new(VecU8Serializer::new()),
        }
    }
}

impl Serializer<AsyncMessageTrigger> for AsyncMessageTriggerSerializer {
    fn serialize(
        &self,
        value: &AsyncMessageTrigger,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.address_serializer.serialize(&value.address, buffer)?;
        self.key_serializer
            .serialize(&value.datastore_key, buffer)?;
        Ok(())
    }
}

/// Deserializer for a trigger for an asynchronous message
struct AsyncMessageTriggerDeserializer {
    address_deserializer: AddressDeserializer,
    key_serializer: OptionDeserializer<Vec<u8>, VecU8Deserializer>,
}

impl AsyncMessageTriggerDeserializer {
    pub(crate) fn new(max_key_length: u32) -> Self {
        Self {
            address_deserializer: AddressDeserializer::new(),
            key_serializer: OptionDeserializer::new(VecU8Deserializer::new(
                Included(0),
                Excluded(max_key_length as u64),
            )),
        }
    }
}

impl Deserializer<AsyncMessageTrigger> for AsyncMessageTriggerDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncMessageTrigger, E> {
        context(
            "Failed AsyncMessageTrigger deserialization",
            tuple((
                context("Failed address deserialization", |input| {
                    self.address_deserializer.deserialize(input)
                }),
                context("Failed datastore_key deserialization", |input| {
                    self.key_serializer.deserialize(input)
                }),
            )),
        )
        .map(|(address, datastore_key)| AsyncMessageTrigger {
            address,
            datastore_key,
        })
        .parse(buffer)
    }
}

/// Structure defining an asynchronous smart contract message
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct AsyncMessage {
    /// Slot at which the message was emitted
    pub(crate) emission_slot: Slot,

    /// Index of the emitted message within the `emission_slot`.
    /// This is used for disambiguate the emission of multiple messages at the same slot.
    pub(crate) emission_index: u64,

    /// The address that sent the message
    pub sender: Address,

    /// The address towards which the message is being sent
    pub destination: Address,

    /// the handler function name within the destination address' bytecode
    pub handler: String,

    /// Maximum gas to use when processing the message
    pub max_gas: u64,

    /// Fee paid by the sender when the message is processed.
    pub(crate) fee: Amount,

    /// Coins sent from the sender to the target address of the message.
    /// Those coins are spent by the sender address when the message is sent,
    /// and credited to the destination address when receiving the message.
    /// In case of failure or discard, those coins are reimbursed to the sender.
    pub coins: Amount,

    /// Slot at which the message starts being valid (bound included in the validity range)
    pub(crate) validity_start: Slot,

    /// Slot at which the message stops being valid (bound not included in the validity range)
    pub(crate) validity_end: Slot,

    /// Raw payload data of the message
    pub data: Vec<u8>,

    /// Trigger that define whenever a message can be executed
    pub(crate) trigger: Option<AsyncMessageTrigger>,

    /// Boolean that determine if the message can be executed. For messages without filter this boolean is always true.
    /// For messages with filter, this boolean is true if the filter has been matched between `validity_start` and current slot.
    pub(crate) can_be_executed: bool,

    /// Hash of the message
    pub(crate) hash: Hash,
}

impl AsyncMessage {
    #[allow(clippy::too_many_arguments)]
    /// Take an `AsyncMessage` and return it with its hash computed
    pub fn new_with_hash(
        emission_slot: Slot,
        emission_index: u64,
        sender: Address,
        destination: Address,
        handler: String,
        max_gas: u64,
        fee: Amount,
        coins: Amount,
        validity_start: Slot,
        validity_end: Slot,
        data: Vec<u8>,
        trigger: Option<AsyncMessageTrigger>,
    ) -> Self {
        let async_message_ser = AsyncMessageSerializer::new();
        let mut buffer = Vec::new();
        let mut message = AsyncMessage {
            emission_slot,
            emission_index,
            sender,
            destination,
            handler,
            max_gas,
            fee,
            coins,
            validity_start,
            validity_end,
            data,
            can_be_executed: trigger.is_none(),
            trigger,
            // placeholder hash to serialize the message, replaced below
            hash: Hash::from_bytes(&[0; 32]),
        };
        async_message_ser
            .serialize(&message, &mut buffer)
            .expect("critical: asynchronous message serialization should never fail here");
        message.hash = Hash::compute_from(&buffer);
        message
    }

    /// Compute the ID of the message for use when choosing which operations to keep in priority (highest score) on pool overflow.
    pub fn compute_id(&self) -> AsyncMessageId {
        let denom = if self.max_gas > 0 { self.max_gas } else { 1 };
        (
            std::cmp::Reverse(Ratio::new(self.fee.to_raw(), denom)),
            self.emission_slot,
            self.emission_index,
        )
    }

    /// Recompute the hash of the message. Must be used each time we modify one field
    pub(crate) fn compute_hash(&mut self) {
        let async_message_ser = AsyncMessageSerializer::new();
        let mut buffer = Vec::new();
        async_message_ser.serialize(self, &mut buffer).expect(
            "critical: asynchronous message serialization should never fail in recompute hash",
        );
        self.hash = Hash::compute_from(&buffer);
    }
}

pub(crate) struct AsyncMessageSerializer {
    slot_serializer: SlotSerializer,
    amount_serializer: AmountSerializer,
    u64_serializer: U64VarIntSerializer,
    vec_u8_serializer: VecU8Serializer,
    address_serializer: AddressSerializer,
    trigger_serializer: OptionSerializer<AsyncMessageTrigger, AsyncMessageTriggerSerializer>,
}

impl AsyncMessageSerializer {
    pub(crate) fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            amount_serializer: AmountSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            address_serializer: AddressSerializer::new(),
            trigger_serializer: OptionSerializer::new(AsyncMessageTriggerSerializer::new()),
        }
    }
}

impl Default for AsyncMessageSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<AsyncMessage> for AsyncMessageSerializer {
    /// ## Example
    /// ```
    /// use massa_async_pool::{AsyncMessage, AsyncMessageSerializer, AsyncMessageTrigger};
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use massa_serialization::Serializer;
    /// use std::str::FromStr;
    ///
    /// let message = AsyncMessage::new_with_hash(
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
    ///     Some(AsyncMessageTrigger {
    ///         address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///         datastore_key: Some(vec![1, 2, 3, 4])
    ///     })
    /// );
    /// let mut buffer = Vec::new();
    /// let message_serializer = AsyncMessageSerializer::new();
    /// message_serializer.serialize(&message, &mut buffer).unwrap();
    /// ```
    fn serialize(
        &self,
        value: &AsyncMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        self.slot_serializer
            .serialize(&value.emission_slot, buffer)?;
        self.u64_serializer
            .serialize(&value.emission_index, buffer)?;
        self.address_serializer.serialize(&value.sender, buffer)?;
        self.address_serializer
            .serialize(&value.destination, buffer)?;

        let handler_bytes = value.handler.as_bytes();
        let handler_name_len: u8 = handler_bytes.len().try_into().map_err(|_| {
            SerializeError::GeneralError("could not convert handler name length to u8".into())
        })?;
        buffer.extend([handler_name_len]);
        buffer.extend(handler_bytes);

        self.u64_serializer.serialize(&value.max_gas, buffer)?;
        self.amount_serializer.serialize(&value.fee, buffer)?;
        self.amount_serializer.serialize(&value.coins, buffer)?;
        self.slot_serializer
            .serialize(&value.validity_start, buffer)?;
        self.slot_serializer
            .serialize(&value.validity_end, buffer)?;
        self.vec_u8_serializer.serialize(&value.data, buffer)?;
        self.trigger_serializer.serialize(&value.trigger, buffer)?;
        Ok(())
    }
}

pub(crate) struct AsyncMessageDeserializer {
    slot_deserializer: SlotDeserializer,
    amount_deserializer: AmountDeserializer,
    emission_index_deserializer: U64VarIntDeserializer,
    max_gas_deserializer: U64VarIntDeserializer,
    data_deserializer: VecU8Deserializer,
    address_deserializer: AddressDeserializer,
    trigger_deserializer: OptionDeserializer<AsyncMessageTrigger, AsyncMessageTriggerDeserializer>,
}

impl AsyncMessageDeserializer {
    pub(crate) fn new(thread_count: u8, max_async_message_data: u64, max_key_length: u32) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            ),
            amount_deserializer: AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            ),
            emission_index_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            ),
            max_gas_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            data_deserializer: VecU8Deserializer::new(
                Included(0),
                Included(max_async_message_data),
            ),
            address_deserializer: AddressDeserializer::new(),
            trigger_deserializer: OptionDeserializer::new(AsyncMessageTriggerDeserializer::new(
                max_key_length,
            )),
        }
    }
}

impl Deserializer<AsyncMessage> for AsyncMessageDeserializer {
    /// ## Example
    /// ```
    /// use massa_async_pool::{AsyncMessage, AsyncMessageSerializer, AsyncMessageDeserializer, AsyncMessageTrigger};
    /// use massa_models::{address::Address, amount::Amount, slot::Slot};
    /// use massa_serialization::{Serializer, Deserializer, DeserializeError};
    /// use std::str::FromStr;
    ///
    /// let message = AsyncMessage::new_with_hash(
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
    ///     Some(AsyncMessageTrigger {
    ///        address: Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
    ///        datastore_key: Some(vec![1, 2, 3, 4]),
    ///     })
    /// );
    /// let message_serializer = AsyncMessageSerializer::new();
    /// let mut serialized = Vec::new();
    /// message_serializer.serialize(&message, &mut serialized).unwrap();
    /// let message_deserializer = AsyncMessageDeserializer::new(32, 100000, 255);
    /// // dbg!(&serialized);
    /// let (rest, message_deserialized) = message_deserializer.deserialize::<DeserializeError>(&serialized).unwrap();
    /// assert!(rest.is_empty());
    /// assert_eq!(message, message_deserialized);
    /// assert_eq!(message.hash, message_deserialized.hash);
    /// ```
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncMessage, E> {
        context(
            "Failed AsyncMessage deserialization",
            tuple((
                context("Failed emission_slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed emission_index deserialization", |input| {
                    self.emission_index_deserializer.deserialize(input)
                }),
                context("Failed sender deserialization", |input| {
                    self.address_deserializer.deserialize(input)
                }),
                context("Failed destination deserialization", |input| {
                    self.address_deserializer.deserialize(input)
                }),
                context("Failed handler deserialization", |input| {
                    let (rest, array) = length_data(|input: &'a [u8]| match input.first() {
                        Some(len) => Ok((&input[1..], *len)),
                        None => Err(nom::Err::Error(ParseError::from_error_kind(
                            input,
                            nom::error::ErrorKind::LengthValue,
                        ))),
                    })(input)?;
                    Ok((
                        rest,
                        String::from_utf8(array.to_vec()).map_err(|_| {
                            nom::Err::Error(ParseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Fail,
                            ))
                        })?,
                    ))
                }),
                context("Failed max_gas deserialization", |input| {
                    self.max_gas_deserializer.deserialize(input)
                }),
                context("Failed fee deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed coins deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed validity_start deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed validity_end deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed data deserialization", |input| {
                    self.data_deserializer.deserialize(input)
                }),
                context("Failed filter deserialization", |input| {
                    self.trigger_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(
                emission_slot,
                emission_index,
                sender,
                destination,
                handler,
                max_gas,
                fee,
                coins,
                validity_start,
                validity_end,
                data,
                filter,
            )| {
                AsyncMessage::new_with_hash(
                    emission_slot,
                    emission_index,
                    sender,
                    destination,
                    handler,
                    max_gas,
                    fee,
                    coins,
                    validity_start,
                    validity_end,
                    data,
                    filter,
                )
            },
        )
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use massa_serialization::{DeserializeError, Deserializer, Serializer};

    use crate::{AsyncMessage, AsyncMessageDeserializer, AsyncMessageSerializer};
    use massa_models::{
        address::Address,
        amount::Amount,
        config::{MAX_ASYNC_MESSAGE_DATA, MAX_DATASTORE_KEY_LENGTH, THREAD_COUNT},
        slot::Slot,
    };
    use std::str::FromStr;

    use super::AsyncMessageTrigger;

    #[test]
    fn bad_serialization_version() {
        let message = AsyncMessage::new_with_hash(
            Slot::new(1, 2),
            0,
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
            String::from("test"),
            10000000,
            Amount::from_str("1").unwrap(),
            Amount::from_str("1").unwrap(),
            Slot::new(2, 0),
            Slot::new(3, 0),
            vec![1, 2, 3, 4],
            Some(AsyncMessageTrigger {
                address: Address::from_str("AU12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G")
                    .unwrap(),
                datastore_key: None,
            }),
        );
        let message_serializer = AsyncMessageSerializer::new();
        let mut serialized = Vec::new();
        message_serializer
            .serialize(&message, &mut serialized)
            .unwrap();
        let message_deserializer = AsyncMessageDeserializer::new(
            THREAD_COUNT,
            MAX_ASYNC_MESSAGE_DATA,
            MAX_DATASTORE_KEY_LENGTH as u32,
        );
        serialized[1] = 50;
        message_deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap_err();
    }
}
