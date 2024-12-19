use massa_models::{
    address::{Address, AddressDeserializer, AddressSerializer},
    amount::{Amount, AmountDeserializer, AmountSerializer},
    serialization::{StringDeserializer, StringSerializer, VecU8Deserializer, VecU8Serializer},
    slot::{Slot, SlotDeserializer, SlotSerializer},
};
use massa_proto_rs::massa::api::v1 as grpc_api;
use massa_serialization::{
    BoolDeserializer, BoolSerializer, Deserializer, SerializeError, Serializer,
    U16VarIntDeserializer, U16VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use serde::{Deserialize, Serialize};
use std::ops::Bound;

use crate::config::DeferredCallsConfig;

/// Definition of a call in the future
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeferredCall {
    // Sender address
    pub sender_address: Address,
    // The slot in which the call will be executed
    pub target_slot: Slot,
    // The address of the contract to call
    pub target_address: Address,
    // The function to call
    pub target_function: String,
    // The parameters of the call
    pub parameters: Vec<u8>,
    // The amount of coins to send to the contract
    pub coins: Amount,
    // The maximum amount of gas usable for the call (excluding the vm allocation cost)
    // to get the effective gas, use get_effective_gas(&self)
    pub max_gas: u64,
    // The fee to pay for the reservation of the space for the call
    pub fee: Amount,
    // Whether the call is cancelled
    pub cancelled: bool,
}

impl DeferredCall {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sender_address: Address,
        target_slot: Slot,
        target_address: Address,
        target_function: String,
        parameters: Vec<u8>,
        coins: Amount,
        max_gas: u64,
        fee: Amount,
        cancelled: bool,
    ) -> Self {
        DeferredCall {
            sender_address,
            target_slot,
            target_address,
            target_function,
            parameters,
            coins,
            max_gas,
            fee,
            cancelled,
        }
    }

    /// Get the effective gas of a call
    /// This is the maximum gas of the call + vm allocation cost
    pub fn get_effective_gas(&self, alloc_gas_cost: u64) -> u64 {
        self.max_gas.saturating_add(alloc_gas_cost)
    }

    /// Get the storage cost for a call
    pub fn get_storage_cost(
        cost_per_byte: Amount,
        params_size: u64,
        max_function_name_length: u16,
    ) -> Amount {
        // 35 (sender_address) + 16 (target_slot) + 35 (target_address) + target_function.len() + params_size + 8 (coins) + 8 (max_gas) + 8 (fee) + 1 (cancelled)
        let total_size = params_size
            .saturating_add(max_function_name_length as u64)
            .saturating_add(111); // 35 + 16 + 35 + 8 + 8 + 8 + 1
        cost_per_byte.saturating_mul_u64(total_size)
    }
}

impl From<DeferredCall> for grpc_api::DeferredCallInfoEntry {
    fn from(call: DeferredCall) -> Self {
        grpc_api::DeferredCallInfoEntry {
            sender_address: call.sender_address.to_string(),
            target_slot: Some(call.target_slot.into()),
            target_address: call.target_address.to_string(),
            target_function: call.target_function,
            parameters: call.parameters,
            coins: Some(call.coins.into()),
            max_gas: call.max_gas,
            fee: Some(call.fee.into()),
            cancelled: call.cancelled,
        }
    }
}

/// Serializer for `AsyncCall`
#[derive(Clone)]
pub struct DeferredCallSerializer {
    pub(crate) slot_serializer: SlotSerializer,
    pub(crate) address_serializer: AddressSerializer,
    pub(crate) string_serializer: StringSerializer<U16VarIntSerializer, u16>,
    pub(crate) vec_u8_serializer: VecU8Serializer,
    pub(crate) amount_serializer: AmountSerializer,
    pub(crate) u64_var_int_serializer: U64VarIntSerializer,
    pub(crate) bool_serializer: BoolSerializer,
}

impl DeferredCallSerializer {
    /// Serializes an `DeferredCall` into a `Vec<u8>`
    pub fn new() -> Self {
        Self {
            slot_serializer: SlotSerializer::new(),
            address_serializer: AddressSerializer::new(),
            string_serializer: StringSerializer::new(U16VarIntSerializer::new()),
            vec_u8_serializer: VecU8Serializer::new(),
            amount_serializer: AmountSerializer::new(),
            u64_var_int_serializer: U64VarIntSerializer::new(),
            bool_serializer: BoolSerializer::new(),
        }
    }
}

impl Serializer<DeferredCall> for DeferredCallSerializer {
    fn serialize(&self, value: &DeferredCall, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        self.address_serializer
            .serialize(&value.sender_address, buffer)?;
        self.slot_serializer.serialize(&value.target_slot, buffer)?;
        self.address_serializer
            .serialize(&value.target_address, buffer)?;
        self.string_serializer
            .serialize(&value.target_function, buffer)?;
        self.vec_u8_serializer
            .serialize(&value.parameters, buffer)?;
        self.amount_serializer.serialize(&value.coins, buffer)?;
        self.u64_var_int_serializer
            .serialize(&value.max_gas, buffer)?;
        self.amount_serializer.serialize(&value.fee, buffer)?;
        self.bool_serializer.serialize(&value.cancelled, buffer)?;
        Ok(())
    }
}

/// Deserializer for `AsyncCall`
#[derive(Clone)]
pub struct DeferredCallDeserializer {
    slot_deserializer: SlotDeserializer,
    address_deserializer: AddressDeserializer,
    string_deserializer: StringDeserializer<U16VarIntDeserializer, u16>,
    vec_u8_deserializer: VecU8Deserializer,
    pub(crate) amount_deserializer: AmountDeserializer,
    pub(crate) u64_var_int_deserializer: U64VarIntDeserializer,
    bool_deserializer: BoolDeserializer,
}

impl DeferredCallDeserializer {
    /// Deserializes a `Vec<u8>` into an `AsyncCall`
    pub fn new(config: DeferredCallsConfig) -> Self {
        Self {
            slot_deserializer: SlotDeserializer::new(
                (Bound::Included(0), Bound::Included(u64::MAX)),
                (Bound::Included(0), Bound::Excluded(config.thread_count)),
            ),
            address_deserializer: AddressDeserializer::new(),
            string_deserializer: StringDeserializer::new(U16VarIntDeserializer::new(
                Bound::Included(0),
                Bound::Included(config.max_function_name_length),
            )),
            vec_u8_deserializer: VecU8Deserializer::new(
                std::ops::Bound::Included(0),
                std::ops::Bound::Included(config.max_parameter_size as u64),
            ),
            amount_deserializer: AmountDeserializer::new(
                Bound::Included(Amount::MIN),
                Bound::Included(Amount::MAX),
            ),
            u64_var_int_deserializer: U64VarIntDeserializer::new(
                Bound::Included(0),
                Bound::Included(u64::MAX),
            ),
            bool_deserializer: BoolDeserializer::new(),
        }
    }
}

impl Deserializer<DeferredCall> for DeferredCallDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], DeferredCall, E> {
        context(
            "Failed AsyncCall deserialization",
            tuple((
                context("Failed sender_address deserialization", |input| {
                    self.address_deserializer.deserialize(input)
                }),
                context("Failed target_slot deserialization", |input| {
                    self.slot_deserializer.deserialize(input)
                }),
                context("Failed target_address deserialization", |input| {
                    self.address_deserializer.deserialize(input)
                }),
                context("Failed target_function deserialization", |input| {
                    self.string_deserializer.deserialize(input)
                }),
                context("Failed parameters deserialization", |input| {
                    self.vec_u8_deserializer.deserialize(input)
                }),
                context("Failed coins deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed max_gas deserialization", |input| {
                    self.u64_var_int_deserializer.deserialize(input)
                }),
                context("Failed fee deserialization", |input| {
                    self.amount_deserializer.deserialize(input)
                }),
                context("Failed cancelled deserialization", |input| {
                    self.bool_deserializer.deserialize(input)
                }),
            )),
        )
        .map(
            |(
                sender_address,
                target_slot,
                target_address,
                target_function,
                parameters,
                coins,
                max_gas,
                fee,
                cancelled,
            )| {
                DeferredCall::new(
                    sender_address,
                    target_slot,
                    target_address,
                    target_function,
                    parameters,
                    coins,
                    max_gas,
                    fee,
                    cancelled,
                )
            },
        )
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use massa_serialization::DeserializeError;

    use super::*;

    #[test]
    fn test_serialization_deserialization() {
        let call = DeferredCall::new(
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            Slot::new(42, 0),
            Address::from_str("AU12dG5xP1RDEB5ocdHkymNVvvSJmUL9BgHwCksDowqmGWxfpm93x").unwrap(),
            "function".to_string(),
            vec![0, 1, 2, 3],
            Amount::from_raw(100),
            500000,
            Amount::from_raw(25),
            false,
        );
        let serializer = DeferredCallSerializer::new();

        let deserializer = DeferredCallDeserializer::new(DeferredCallsConfig::default());
        let mut buffer = Vec::new();
        serializer.serialize(&call, &mut buffer).unwrap();
        let (rest, deserialized_call) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();
        assert_eq!(call, deserialized_call);
        assert!(rest.is_empty());
    }
}
