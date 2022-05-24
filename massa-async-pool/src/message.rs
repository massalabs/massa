//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines the structure representing an asynchronous message

use std::ops::Bound::Included;

use massa_models::amount::{AmountDeserializer, AmountSerializer};
use massa_models::constants::{ADDRESS_SIZE_BYTES, THREAD_COUNT};
use massa_models::slot::{SlotDeserializer, SlotSerializer};
use massa_models::{
    array_from_slice, Address, Amount, DeserializeVarInt, ModelsError, SerializeVarInt, Slot,
    U64VarIntDeserializer, U64VarIntSerializer,
};
use massa_models::{DeserializeCompact, SerializeCompact};
use massa_serialization::{Deserializer, Serializer};
use nom::error::context;
use nom::sequence::tuple;
use nom::IResult;
use serde::{Deserialize, Serialize};

/// Unique identifier of a message.
/// Also has the property of ordering by priority (highest first) following the triplet:
/// `(rev(max_gas*gas_price), emission_slot, emission_index)`
pub type AsyncMessageId = (std::cmp::Reverse<Amount>, Slot, u64);

pub struct AsyncMessageIdSerializer {
    amount_serializer: AmountSerializer,
    slot_serializer: SlotSerializer,
    u64_serializer: U64VarIntSerializer,
}

impl AsyncMessageIdSerializer {
    pub fn new() -> Self {
        Self {
            amount_serializer: AmountSerializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_serializer: SlotSerializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
            u64_serializer: U64VarIntSerializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Serializer<AsyncMessageId> for AsyncMessageIdSerializer {
    fn serialize(
        &self,
        value: &AsyncMessageId,
    ) -> Result<Vec<u8>, massa_serialization::SerializeError> {
        let amount = self.amount_serializer.serialize(&value.0 .0)?;
        let slot = self.slot_serializer.serialize(&value.1)?;
        let index = self.u64_serializer.serialize(&value.2)?;
        let mut res = Vec::with_capacity(amount.len() + slot.len() + index.len());
        res.extend(amount);
        res.extend(slot);
        res.extend(index);
        Ok(res)
    }
}

pub struct AsyncMessageIdDeserializer {
    amount_deserializer: AmountDeserializer,
    slot_deserializer: SlotDeserializer,
    u64_deserializer: U64VarIntDeserializer,
}

impl AsyncMessageIdDeserializer {
    pub fn new() -> Self {
        Self {
            amount_deserializer: AmountDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
            slot_deserializer: SlotDeserializer::new(
                (Included(u64::MIN), Included(u64::MAX)),
                (Included(0), Included(THREAD_COUNT)),
            ),
            u64_deserializer: U64VarIntDeserializer::new(Included(u64::MIN), Included(u64::MAX)),
        }
    }
}

impl Deserializer<AsyncMessageId> for AsyncMessageIdDeserializer {
    fn deserialize<'a>(&self, buffer: &'a [u8]) -> IResult<&'a [u8], AsyncMessageId> {
        let parser = tuple((
            context("amount in async message id", |input| {
                self.amount_deserializer.deserialize(input)
            }),
            context("slot in async message id", |input| {
                self.slot_deserializer.deserialize(input)
            }),
            context("index in async message id", |input| {
                self.u64_deserializer.deserialize(input)
            }),
        ));

        match parser(buffer) {
            Ok((rest, (amount, slot, index))) => {
                Ok((rest, (std::cmp::Reverse(amount), slot, index)))
            }
            Err(e) => Err(e),
        }
    }
}

/// Structure defining an asynchronous smart contract message
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Gas price to take into account when executing the message.
    /// `max_gas * gas_price` are burned by the sender when the message is sent.
    pub gas_price: Amount,

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
}

impl AsyncMessage {
    /// Compute the ID of the message for use when choosing which operations to keep in priority (highest score) on pool overflow.
    /// For now, the formula is simply `score = (gas_price * max_gas, rev(emission_slot), rev(emission_index))`
    pub fn compute_id(&self) -> AsyncMessageId {
        (
            std::cmp::Reverse(self.gas_price.saturating_mul_u64(self.max_gas)),
            self.emission_slot,
            self.emission_index,
        )
    }
}

impl SerializeCompact for AsyncMessage {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, massa_models::ModelsError> {
        let mut res: Vec<u8> = Vec::new();

        // emission slot
        res.extend(self.emission_slot.to_bytes_compact()?);

        // emission index
        res.extend(self.emission_index.to_varint_bytes());

        // sender address
        res.extend(self.sender.to_bytes());

        // destination address
        res.extend(self.destination.to_bytes());

        // handler name length
        let handler_name_len: u8 = self.handler.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert handler name length to u8".into())
        })?;
        res.extend(&[handler_name_len]);

        // handler name
        res.extend(self.handler.as_bytes());

        // max gas
        res.extend(&self.max_gas.to_varint_bytes());

        // gas price
        res.extend(&self.gas_price.to_bytes_compact()?);

        // coins
        res.extend(&self.coins.to_bytes_compact()?);

        // validity_start
        res.extend(&self.validity_start.to_bytes_compact()?);

        // validity_end
        res.extend(&self.validity_end.to_bytes_compact()?);

        // data length
        let data_len: u64 = self.data.len().try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert data size to u64".into())
        })?;
        res.extend(data_len.to_varint_bytes());

        // data
        res.extend(&self.data);

        Ok(res)
    }
}

impl DeserializeCompact for AsyncMessage {
    fn from_bytes_compact(buffer: &[u8]) -> Result<(Self, usize), massa_models::ModelsError> {
        let mut cursor = 0usize;

        // emission slot
        let (emission_slot, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // emission index
        let (emission_index, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // sender address
        let sender = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += ADDRESS_SIZE_BYTES;

        // destination address
        let destination = Address::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
        cursor += ADDRESS_SIZE_BYTES;

        // handler name length
        let handler_name_len = *buffer
            .get(cursor)
            .ok_or_else(|| ModelsError::SerializeError("buffer ended prematurely".into()))?;
        cursor += 1;

        // handler name
        let handler = if let Some(range) = buffer.get(cursor..(cursor + handler_name_len as usize))
        {
            cursor += handler_name_len as usize;
            String::from_utf8(range.to_vec())
                .map_err(|_| ModelsError::SerializeError("handler name is not utf-8".into()))?
        } else {
            return Err(ModelsError::SerializeError(
                "buffer ended prematurely".into(),
            ));
        };

        // max gas
        let (max_gas, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        // gas price
        let (gas_price, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // coins
        let (coins, delta) = Amount::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // validity_start
        let (validity_start, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // validity_end
        let (validity_end, delta) = Slot::from_bytes_compact(&buffer[cursor..])?;
        cursor += delta;

        // data length
        let (data_len, delta) = u64::from_varint_bytes(&buffer[cursor..])?;
        let data_len: usize = data_len.try_into().map_err(|_| {
            ModelsError::SerializeError("could not convert data size to usize".into())
        })?;
        //TODO cap data length https://github.com/massalabs/massa/issues/1200
        cursor += delta;

        // data
        let data = if let Some(slice) = buffer.get(cursor..(cursor + data_len)) {
            cursor += data_len as usize;
            slice.to_vec()
        } else {
            return Err(ModelsError::SerializeError(
                "buffer ended prematurely".into(),
            ));
        };

        Ok((
            AsyncMessage {
                emission_slot,
                emission_index,
                sender,
                destination,
                handler,
                max_gas,
                gas_price,
                coins,
                validity_start,
                validity_end,
                data,
            },
            cursor,
        ))
    }
}
