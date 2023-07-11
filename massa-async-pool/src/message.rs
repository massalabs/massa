//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the structure representing an asynchronous message

use massa_hash::{Hash, HashDeserializer, HashSerializer};
use massa_ledger_exports::{Applicable, SetOrKeep, SetOrKeepDeserializer, SetOrKeepSerializer};
use massa_models::address::{AddressDeserializer, AddressSerializer};
use massa_models::amount::{AmountDeserializer, AmountSerializer};
use massa_models::config::GENESIS_KEY;
use massa_models::slot::{SlotDeserializer, SlotSerializer};
use massa_models::{
    address::Address,
    amount::Amount,
    serialization::{VecU8Deserializer, VecU8Serializer},
    slot::Slot,
};
use massa_serialization::{
    BoolDeserializer, BoolSerializer, Deserializer, OptionDeserializer, OptionSerializer,
    SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
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

#[derive(Clone)]
/// Serializer for a trigger for an asynchronous message
pub struct AsyncMessageTriggerSerializer {
    address_serializer: AddressSerializer,
    key_serializer: OptionSerializer<Vec<u8>, VecU8Serializer>,
}

impl AsyncMessageTriggerSerializer {
    pub fn new() -> Self {
        Self {
            address_serializer: AddressSerializer::new(),
            key_serializer: OptionSerializer::new(VecU8Serializer::new()),
        }
    }
}

impl Default for AsyncMessageTriggerSerializer {
    fn default() -> Self {
        Self::new()
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

#[derive(Clone)]
/// Deserializer for a trigger for an asynchronous message
pub struct AsyncMessageTriggerDeserializer {
    address_deserializer: AddressDeserializer,
    key_serializer: OptionDeserializer<Vec<u8>, VecU8Deserializer>,
}

impl AsyncMessageTriggerDeserializer {
    pub fn new(max_key_length: u32) -> Self {
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
    pub emission_slot: Slot,

    /// Index of the emitted message within the `emission_slot`.
    /// This is used for disambiguate the emission of multiple messages at the same slot.
    pub emission_index: u64,

    /// The address that sent the message
    pub sender: Address,

    /// The address towards which the message is being sent
    pub destination: Address,

    /// the handler function name within the destination address' bytecode
    pub handler: String,

    /// Maximum gas to use when processing the message
    pub max_gas: u64,

    /// Fee paid by the sender when the message is processed.
    pub fee: Amount,

    /// Coins sent from the sender to the target address of the message.
    /// Those coins are spent by the sender address when the message is sent,
    /// and credited to the destination address when receiving the message.
    /// In case of failure or discard, those coins are reimbursed to the sender.
    pub coins: Amount,

    /// Slot at which the message starts being valid (bound included in the validity range)
    pub validity_start: Slot,

    /// Slot at which the message stops being valid (bound not included in the validity range)
    pub validity_end: Slot,

    /// Raw payload data of the message
    pub data: Vec<u8>,

    /// Trigger that define whenever a message can be executed
    pub trigger: Option<AsyncMessageTrigger>,

    /// Boolean that determine if the message can be executed. For messages without filter this boolean is always true.
    /// For messages with filter, this boolean is true if the filter has been matched between `validity_start` and current slot.
    pub can_be_executed: bool,

    /// Hash of the message
    pub hash: Hash,
}

impl Default for AsyncMessage {
    #[allow(unconditional_recursion)]
    fn default() -> Self {
        let genesis_address = Address::from_public_key(&(*GENESIS_KEY).get_public_key());
        let slot_zero = Slot::new(0, 0);
        Self {
            emission_slot: slot_zero,
            sender: genesis_address,
            destination: genesis_address,
            validity_start: slot_zero,
            validity_end: slot_zero,
            hash: Hash::zero(),
            ..Default::default()
        }
    }
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
        can_be_executed: Option<bool>,
    ) -> Self {
        let async_message_ser = AsyncMessageSerializer::new(can_be_executed.is_some());
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
            can_be_executed: can_be_executed.unwrap_or(trigger.is_none()),
            trigger,
            // placeholder hash to serialize the message, replaced below
            hash: Hash::zero(),
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
    pub fn compute_hash(&mut self, for_db: bool) {
        let async_message_ser = AsyncMessageSerializer::new(for_db);
        let mut buffer = Vec::new();
        async_message_ser.serialize(self, &mut buffer).expect(
            "critical: asynchronous message serialization should never fail in recompute hash",
        );
        self.hash = Hash::compute_from(&buffer);
    }
}

#[derive(Clone)]
pub struct AsyncMessageSerializer {
    pub slot_serializer: SlotSerializer,
    pub amount_serializer: AmountSerializer,
    pub u64_serializer: U64VarIntSerializer,
    pub vec_u8_serializer: VecU8Serializer,
    pub address_serializer: AddressSerializer,
    pub trigger_serializer: OptionSerializer<AsyncMessageTrigger, AsyncMessageTriggerSerializer>,
    pub bool_serializer: BoolSerializer,
    pub for_db: bool,
}

impl AsyncMessageSerializer {
    pub fn new(for_db: bool) -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            amount_serializer: AmountSerializer::new(),
            u64_serializer: U64VarIntSerializer::new(),
            vec_u8_serializer: VecU8Serializer::new(),
            address_serializer: AddressSerializer::new(),
            trigger_serializer: OptionSerializer::new(AsyncMessageTriggerSerializer::new()),
            bool_serializer: BoolSerializer::new(),
            for_db,
        }
    }
}

impl Default for AsyncMessageSerializer {
    fn default() -> Self {
        Self::new(false)
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
    ///     }),
    ///     None,
    /// );
    /// let mut buffer = Vec::new();
    /// let message_serializer = AsyncMessageSerializer::new(false);
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
        if self.for_db {
            self.bool_serializer
                .serialize(&value.can_be_executed, buffer)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct AsyncMessageDeserializer {
    pub slot_deserializer: SlotDeserializer,
    pub amount_deserializer: AmountDeserializer,
    pub emission_index_deserializer: U64VarIntDeserializer,
    pub max_gas_deserializer: U64VarIntDeserializer,
    pub data_deserializer: VecU8Deserializer,
    pub address_deserializer: AddressDeserializer,
    pub trigger_deserializer:
        OptionDeserializer<AsyncMessageTrigger, AsyncMessageTriggerDeserializer>,
    pub bool_deserializer: BoolDeserializer,
    pub for_db: bool,
}

impl AsyncMessageDeserializer {
    pub fn new(
        thread_count: u8,
        max_async_message_data: u64,
        max_key_length: u32,
        for_db: bool,
    ) -> Self {
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
            bool_deserializer: BoolDeserializer::new(),
            for_db,
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
    ///     Address::from_str("AS12htxRWiEm8jDJpJptr6cwEhWNcCSFWstN1MLSa96DDkVM9Y42G").unwrap(),
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
    ///     }),
    ///     None,
    /// );
    /// let message_serializer = AsyncMessageSerializer::new(false);
    /// let mut serialized = Vec::new();
    /// message_serializer.serialize(&message, &mut serialized).unwrap();
    /// let message_deserializer = AsyncMessageDeserializer::new(32, 100000, 255, false);
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
                context("Failed can_be_executed deserialization", |input| {
                    if self.for_db {
                        self.bool_deserializer.deserialize(input)
                    } else {
                        Ok((input, false))
                    }
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
                can_be_executed,
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
                    if self.for_db {
                        Some(can_be_executed)
                    } else {
                        None
                    },
                )
            },
        )
        .parse(buffer)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncMessageInfo {
    pub validity_start: Slot,
    pub validity_end: Slot,
    pub max_gas: u64,
    pub can_be_executed: bool,
    pub trigger: Option<AsyncMessageTrigger>,
}

impl From<AsyncMessage> for AsyncMessageInfo {
    fn from(value: AsyncMessage) -> Self {
        Self {
            validity_start: value.validity_start,
            validity_end: value.validity_end,
            max_gas: value.max_gas,
            can_be_executed: value.can_be_executed,
            trigger: value.trigger,
        }
    }
}

/// represents an update to one or more fields of a `AsyncMessage`
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AsyncMessageUpdate {
    /// Slot at which the message was emitted
    pub emission_slot: SetOrKeep<Slot>,

    /// Index of the emitted message within the `emission_slot`.
    /// This is used for disambiguate the emission of multiple messages at the same slot.
    pub emission_index: SetOrKeep<u64>,

    /// The address that sent the message
    pub sender: SetOrKeep<Address>,

    /// The address towards which the message is being sent
    pub destination: SetOrKeep<Address>,

    /// the handler function name within the destination address' bytecode
    pub handler: SetOrKeep<String>,

    /// Maximum gas to use when processing the message
    pub max_gas: SetOrKeep<u64>,

    /// Fee paid by the sender when the message is processed.
    pub fee: SetOrKeep<Amount>,

    /// Coins sent from the sender to the target address of the message.
    /// Those coins are spent by the sender address when the message is sent,
    /// and credited to the destination address when receiving the message.
    /// In case of failure or discard, those coins are reimbursed to the sender.
    pub coins: SetOrKeep<Amount>,

    /// Slot at which the message starts being valid (bound included in the validity range)
    pub validity_start: SetOrKeep<Slot>,

    /// Slot at which the message stops being valid (bound not included in the validity range)
    pub validity_end: SetOrKeep<Slot>,

    /// Raw payload data of the message
    pub data: SetOrKeep<Vec<u8>>,

    /// Trigger that define whenever a message can be executed
    pub trigger: SetOrKeep<Option<AsyncMessageTrigger>>,

    /// Boolean that determine if the message can be executed. For messages without filter this boolean is always true.
    /// For messages with filter, this boolean is true if the filter has been matched between `validity_start` and current slot.
    pub can_be_executed: SetOrKeep<bool>,

    /// Hash of the message
    pub hash: SetOrKeep<Hash>,
}

/// Serializer for `AsyncMessageUpdate`
pub struct AsyncMessageUpdateSerializer {
    slot_serializer: SetOrKeepSerializer<Slot, SlotSerializer>,
    amount_serializer: SetOrKeepSerializer<Amount, AmountSerializer>,
    u64_serializer: SetOrKeepSerializer<u64, U64VarIntSerializer>,
    vec_u8_serializer: SetOrKeepSerializer<Vec<u8>, VecU8Serializer>,
    address_serializer: SetOrKeepSerializer<Address, AddressSerializer>,
    trigger_serializer: SetOrKeepSerializer<
        Option<AsyncMessageTrigger>,
        OptionSerializer<AsyncMessageTrigger, AsyncMessageTriggerSerializer>,
    >,
    bool_serializer: SetOrKeepSerializer<bool, BoolSerializer>,
    regular_bool_serializer: BoolSerializer,
    hash_serializer: SetOrKeepSerializer<Hash, HashSerializer>,
    for_db: bool,
}

impl AsyncMessageUpdateSerializer {
    /// Creates a new `AsyncMessageUpdateSerializer`
    pub fn new(for_db: bool) -> Self {
        Self {
            slot_serializer: SetOrKeepSerializer::new(SlotSerializer::new()),
            amount_serializer: SetOrKeepSerializer::new(AmountSerializer::new()),
            u64_serializer: SetOrKeepSerializer::new(U64VarIntSerializer::new()),
            vec_u8_serializer: SetOrKeepSerializer::new(VecU8Serializer::new()),
            address_serializer: SetOrKeepSerializer::new(AddressSerializer::new()),
            trigger_serializer: SetOrKeepSerializer::new(OptionSerializer::new(
                AsyncMessageTriggerSerializer::new(),
            )),
            bool_serializer: SetOrKeepSerializer::new(BoolSerializer::new()),
            regular_bool_serializer: BoolSerializer::new(),
            hash_serializer: SetOrKeepSerializer::new(HashSerializer::new()),
            for_db,
        }
    }
}

impl Default for AsyncMessageUpdateSerializer {
    fn default() -> Self {
        Self::new(false)
    }
}

impl Serializer<AsyncMessageUpdate> for AsyncMessageUpdateSerializer {
    fn serialize(
        &self,
        value: &AsyncMessageUpdate,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        self.slot_serializer
            .serialize(&value.emission_slot, buffer)?;
        self.u64_serializer
            .serialize(&value.emission_index, buffer)?;
        self.address_serializer.serialize(&value.sender, buffer)?;
        self.address_serializer
            .serialize(&value.destination, buffer)?;

        match &value.handler {
            SetOrKeep::Keep => {
                self.regular_bool_serializer.serialize(&false, buffer)?;
            }
            SetOrKeep::Set(s) => {
                let handler_bytes = s.as_bytes();
                let handler_name_len: u8 = handler_bytes.len().try_into().map_err(|_| {
                    SerializeError::GeneralError(
                        "could not convert handler name length to u8".into(),
                    )
                })?;
                self.regular_bool_serializer.serialize(&true, buffer)?;
                buffer.extend([handler_name_len]);
                buffer.extend(handler_bytes);
            }
        };

        self.u64_serializer.serialize(&value.max_gas, buffer)?;
        self.amount_serializer.serialize(&value.fee, buffer)?;
        self.amount_serializer.serialize(&value.coins, buffer)?;
        self.slot_serializer
            .serialize(&value.validity_start, buffer)?;
        self.slot_serializer
            .serialize(&value.validity_end, buffer)?;
        self.vec_u8_serializer.serialize(&value.data, buffer)?;
        self.trigger_serializer.serialize(&value.trigger, buffer)?;
        if self.for_db {
            self.bool_serializer
                .serialize(&value.can_be_executed, buffer)?;
        }
        self.hash_serializer.serialize(&value.hash, buffer)?;
        Ok(())
    }
}

/// Deserializer for `AsyncMessageUpdate`
pub struct AsyncMessageUpdateDeserializer {
    slot_deserializer: SetOrKeepDeserializer<Slot, SlotDeserializer>,
    amount_deserializer: SetOrKeepDeserializer<Amount, AmountDeserializer>,
    emission_index_deserializer: SetOrKeepDeserializer<u64, U64VarIntDeserializer>,
    max_gas_deserializer: SetOrKeepDeserializer<u64, U64VarIntDeserializer>,
    data_deserializer: SetOrKeepDeserializer<Vec<u8>, VecU8Deserializer>,
    address_deserializer: SetOrKeepDeserializer<Address, AddressDeserializer>,
    trigger_deserializer: SetOrKeepDeserializer<
        Option<AsyncMessageTrigger>,
        OptionDeserializer<AsyncMessageTrigger, AsyncMessageTriggerDeserializer>,
    >,
    bool_deserializer: SetOrKeepDeserializer<bool, BoolDeserializer>,
    regular_bool_deserializer: BoolDeserializer,
    hash_deserializer: SetOrKeepDeserializer<Hash, HashDeserializer>,
    for_db: bool,
}

impl AsyncMessageUpdateDeserializer {
    /// Creates a new `AsyncMessageUpdateDeserializer`
    pub fn new(
        thread_count: u8,
        max_async_message_data: u64,
        max_key_length: u32,
        for_db: bool,
    ) -> Self {
        Self {
            slot_deserializer: SetOrKeepDeserializer::new(SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(thread_count)),
            )),
            amount_deserializer: SetOrKeepDeserializer::new(AmountDeserializer::new(
                Included(Amount::MIN),
                Included(Amount::MAX),
            )),
            emission_index_deserializer: SetOrKeepDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            )),
            max_gas_deserializer: SetOrKeepDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(u64::MAX),
            )),
            data_deserializer: SetOrKeepDeserializer::new(VecU8Deserializer::new(
                Included(0),
                Included(max_async_message_data),
            )),
            address_deserializer: SetOrKeepDeserializer::new(AddressDeserializer::new()),
            trigger_deserializer: SetOrKeepDeserializer::new(OptionDeserializer::new(
                AsyncMessageTriggerDeserializer::new(max_key_length),
            )),
            bool_deserializer: SetOrKeepDeserializer::new(BoolDeserializer::new()),
            regular_bool_deserializer: BoolDeserializer::new(),
            hash_deserializer: SetOrKeepDeserializer::new(HashDeserializer::new()),
            for_db,
        }
    }
}

impl Deserializer<AsyncMessageUpdate> for AsyncMessageUpdateDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], AsyncMessageUpdate, E> {
        context(
            "Failed AsyncMessageUpdate deserialization",
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
                    let (rest, id) = self.regular_bool_deserializer.deserialize(input)?;
                    if id {
                        let (rest, array) = length_data(|input: &'a [u8]| match input.first() {
                            Some(len) => Ok((&input[1..], *len)),
                            None => Err(nom::Err::Error(ParseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::LengthValue,
                            ))),
                        })(rest)?;
                        Ok((
                            rest,
                            SetOrKeep::Set(String::from_utf8(array.to_vec()).map_err(|_| {
                                nom::Err::Error(ParseError::from_error_kind(
                                    input,
                                    nom::error::ErrorKind::Fail,
                                ))
                            })?),
                        ))
                    } else {
                        Ok((rest, SetOrKeep::Keep))
                    }
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
                context("Failed can_be_executed deserialization", |input| {
                    if self.for_db {
                        self.bool_deserializer.deserialize(input)
                    } else {
                        Ok((input, SetOrKeep::Keep))
                    }
                }),
                context("Failed hash deserialization", |input| {
                    self.hash_deserializer.deserialize(input)
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
                trigger,
                can_be_executed,
                hash,
            )| {
                AsyncMessageUpdate {
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
                    trigger,
                    can_be_executed,
                    hash,
                }
            },
        )
        .parse(buffer)
    }
}

impl Applicable<AsyncMessageUpdate> for AsyncMessageUpdate {
    /// extends the `AsyncMessageUpdate` with another one
    fn apply(&mut self, update: AsyncMessageUpdate) {
        self.emission_slot.apply(update.emission_slot);
        self.emission_index.apply(update.emission_index);
        self.sender.apply(update.sender);
        self.destination.apply(update.destination);
        self.handler.apply(update.handler);
        self.max_gas.apply(update.max_gas);
        self.fee.apply(update.fee);
        self.coins.apply(update.coins);
        self.validity_start.apply(update.validity_start);
        self.validity_end.apply(update.validity_end);
        self.data.apply(update.data);
        self.trigger.apply(update.trigger);
        self.can_be_executed.apply(update.can_be_executed);
        self.hash.apply(update.hash);
    }
}

impl Applicable<AsyncMessageUpdate> for AsyncMessage {
    /// extends the `AsyncMessage` with a `AsyncMessageUpdate`
    fn apply(&mut self, update: AsyncMessageUpdate) {
        update.emission_slot.apply_to(&mut self.emission_slot);
        update.emission_index.apply_to(&mut self.emission_index);
        update.sender.apply_to(&mut self.sender);
        update.destination.apply_to(&mut self.destination);
        update.handler.apply_to(&mut self.handler);
        update.max_gas.apply_to(&mut self.max_gas);
        update.fee.apply_to(&mut self.fee);
        update.coins.apply_to(&mut self.coins);
        update.validity_start.apply_to(&mut self.validity_start);
        update.validity_end.apply_to(&mut self.validity_end);
        update.data.apply_to(&mut self.data);
        update.trigger.apply_to(&mut self.trigger);
        update.can_be_executed.apply_to(&mut self.can_be_executed);
        update.hash.apply_to(&mut self.hash);
    }
}

impl Applicable<AsyncMessageUpdate> for AsyncMessageInfo {
    /// extends the `AsyncMessage` with a `AsyncMessageUpdate`
    fn apply(&mut self, update: AsyncMessageUpdate) {
        update.max_gas.apply_to(&mut self.max_gas);
        update.validity_start.apply_to(&mut self.validity_start);
        update.validity_end.apply_to(&mut self.validity_end);
        update.trigger.apply_to(&mut self.trigger);
        update.can_be_executed.apply_to(&mut self.can_be_executed);
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
            None,
        );
        let message_serializer = AsyncMessageSerializer::new(false);
        let mut serialized = Vec::new();
        message_serializer
            .serialize(&message, &mut serialized)
            .unwrap();
        let message_deserializer = AsyncMessageDeserializer::new(
            THREAD_COUNT,
            MAX_ASYNC_MESSAGE_DATA,
            MAX_DATASTORE_KEY_LENGTH as u32,
            false,
        );
        serialized[1] = 50;
        message_deserializer
            .deserialize::<DeserializeError>(&serialized)
            .unwrap_err();
    }
}
