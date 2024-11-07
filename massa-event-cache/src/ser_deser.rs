use std::collections::Bound::{Excluded, Included};
use std::collections::VecDeque;
// third-party
use nom::{
    bytes::complete::take,
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
// internal
use massa_models::address::{AddressDeserializer, AddressSerializer};
use massa_models::block_id::{BlockId, BlockIdDeserializer, BlockIdSerializer};
use massa_models::operation::{OperationId, OperationIdDeserializer, OperationIdSerializer};
use massa_models::output_event::{EventExecutionContext, SCOutputEvent};
use massa_models::serialization::{StringDeserializer, StringSerializer};
use massa_models::slot::{SlotDeserializer, SlotSerializer};
use massa_serialization::{
    Deserializer, OptionDeserializer, OptionSerializer, SerializeError, Serializer,
    U32VarIntDeserializer, U32VarIntSerializer, U64VarIntDeserializer, U64VarIntSerializer,
};

/// Metadata serializer
pub struct SCOutputEventSerializer {
    index_in_slot_ser: U64VarIntSerializer,
    addr_len_ser: U32VarIntSerializer,
    slot_ser: SlotSerializer,
    addr_ser: AddressSerializer,
    block_id_ser: OptionSerializer<BlockId, BlockIdSerializer>,
    op_id_ser: OptionSerializer<OperationId, OperationIdSerializer>,
    data_ser: StringSerializer<U64VarIntSerializer, u64>,
}

impl SCOutputEventSerializer {
    pub fn new() -> Self {
        Self {
            index_in_slot_ser: U64VarIntSerializer::new(),
            addr_len_ser: U32VarIntSerializer::new(),
            slot_ser: SlotSerializer::new(),
            addr_ser: AddressSerializer::new(),
            block_id_ser: OptionSerializer::new(BlockIdSerializer::new()),
            op_id_ser: OptionSerializer::new(OperationIdSerializer::new()),
            data_ser: StringSerializer::new(U64VarIntSerializer::new()),
        }
    }
}

impl Default for SCOutputEventSerializer {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializer<SCOutputEvent> for SCOutputEventSerializer {
    fn serialize(&self, value: &SCOutputEvent, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        // context
        self.slot_ser.serialize(&value.context.slot, buffer)?;
        self.block_id_ser.serialize(&value.context.block, buffer)?;
        buffer.push(u8::from(value.context.read_only));
        self.index_in_slot_ser
            .serialize(&value.context.index_in_slot, buffer)?;
        // Components
        let call_stack_len_ = value.context.call_stack.len();
        let call_stack_len = u32::try_from(call_stack_len_).map_err(|_| {
            SerializeError::GeneralError(format!(
                "Cannot convert component_len ({}) to u32",
                call_stack_len_
            ))
        })?;
        // ser vec len
        self.addr_len_ser.serialize(&call_stack_len, buffer)?;
        for address in value.context.call_stack.iter() {
            self.addr_ser.serialize(address, buffer)?;
        }
        self.op_id_ser
            .serialize(&value.context.origin_operation_id, buffer)?;
        buffer.push(u8::from(value.context.is_final));
        buffer.push(u8::from(value.context.is_error));

        // data
        self.data_ser.serialize(&value.data, buffer)?;
        Ok(())
    }
}

/// SCOutputEvent deserializer
pub struct SCOutputEventDeserializer {
    index_in_slot_deser: U64VarIntDeserializer,
    addr_len_deser: U32VarIntDeserializer,
    slot_deser: SlotDeserializer,
    addr_deser: AddressDeserializer,
    block_id_deser: OptionDeserializer<BlockId, BlockIdDeserializer>,
    op_id_deser: OptionDeserializer<OperationId, OperationIdDeserializer>,
    data_deser: StringDeserializer<U64VarIntDeserializer, u64>,
}

impl SCOutputEventDeserializer {
    pub fn new(args: SCOutputEventDeserializerArgs) -> Self {
        Self {
            index_in_slot_deser: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            addr_len_deser: U32VarIntDeserializer::new(
                Included(0),
                Included(u32::from(args.max_call_stack_length)),
            ),
            slot_deser: SlotDeserializer::new(
                (Included(0), Included(u64::MAX)),
                (Included(0), Excluded(args.thread_count)),
            ),
            addr_deser: Default::default(),
            block_id_deser: OptionDeserializer::new(BlockIdDeserializer::new()),
            op_id_deser: OptionDeserializer::new(OperationIdDeserializer::new()),
            data_deser: StringDeserializer::new(U64VarIntDeserializer::new(
                Included(0),
                Included(args.max_event_data_length),
            )),
        }
    }
}

impl Deserializer<SCOutputEvent> for SCOutputEventDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], SCOutputEvent, E> {
        context(
            "Failed ScOutputEvent deserialization",
            tuple((
                context("Failed slot deserialization", |input| {
                    self.slot_deser.deserialize(input)
                }),
                context("Failed BlockId deserialization", |input| {
                    self.block_id_deser.deserialize(input)
                }),
                context("Failed read_only deserialization", |input: &'a [u8]| {
                    let (rem, read_only) = take(1usize)(input)?;
                    let read_only = match read_only.first() {
                        None => {
                            return IResult::Err(nom::Err::Error(ParseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Fail,
                            )));
                        }
                        Some(0) => false,
                        _ => true,
                    };
                    IResult::Ok((rem, read_only))
                }),
                context("Failed index_in_slot deser", |input| {
                    self.index_in_slot_deser.deserialize(input)
                }),
                length_count(
                    context("Failed call stack entry count deser", |input| {
                        self.addr_len_deser.deserialize(input)
                    }),
                    context("Failed call stack items deser", |input| {
                        self.addr_deser.deserialize(input)
                    }),
                ),
                context("Failed OperationId deserialization", |input| {
                    self.op_id_deser.deserialize(input)
                }),
                context("Failed is_final deserialization", |input: &'a [u8]| {
                    let (rem, read_only) = take(1usize)(input)?;
                    let read_only = match read_only.first() {
                        None => {
                            return IResult::Err(nom::Err::Error(ParseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Fail,
                            )));
                        }
                        Some(0) => false,
                        _ => true,
                    };
                    IResult::Ok((rem, read_only))
                }),
                context("Failed is_error deserialization", |input: &'a [u8]| {
                    let (rem, read_only) = take(1usize)(input)?;
                    let read_only = match read_only.first() {
                        None => {
                            return IResult::Err(nom::Err::Error(ParseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Fail,
                            )));
                        }
                        Some(0) => false,
                        _ => true,
                    };
                    IResult::Ok((rem, read_only))
                }),
                context("Failed data deserialization", |input| {
                    self.data_deser.deserialize(input)
                }),
            )),
        )
        .map(
            |(slot, bid, read_only, idx, call_stack, oid, is_final, is_error, data)| {
                SCOutputEvent {
                    context: EventExecutionContext {
                        slot,
                        block: bid,
                        read_only,
                        index_in_slot: idx,
                        call_stack: VecDeque::from(call_stack),
                        origin_operation_id: oid,
                        is_final,
                        is_error,
                    },
                    data,
                }
            },
        )
        .parse(buffer)
    }
}

/// SCOutputEvent deserializer args
#[allow(missing_docs)]
pub struct SCOutputEventDeserializerArgs {
    pub thread_count: u8,
    pub max_call_stack_length: u16,
    pub max_event_data_length: u64,
}

#[cfg(test)]
mod test {
    use super::*;
    use massa_models::slot::Slot;
    use massa_serialization::DeserializeError;
    use serial_test::serial;

    #[test]
    #[serial]
    fn test_sc_output_event_ser_der() {
        let slot_1 = Slot::new(1, 0);
        let index_1_0 = 0;
        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: slot_1,
                block: None,
                read_only: false,
                index_in_slot: index_1_0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        let event_ser = SCOutputEventSerializer::new();
        let event_deser = SCOutputEventDeserializer::new(SCOutputEventDeserializerArgs {
            thread_count: 16,
            max_call_stack_length: 25,
            max_event_data_length: 512,
        });

        let mut buffer = Vec::new();
        event_ser.serialize(&event, &mut buffer).unwrap();

        let (rem, event_new) = event_deser
            .deserialize::<DeserializeError>(&buffer)
            .unwrap();

        assert_eq!(event.context, event_new.context);
        assert_eq!(event.data, event_new.data);
        assert!(rem.is_empty());
    }

    #[test]
    #[serial]
    fn test_sc_output_event_ser_der_err() {
        // Test serialization / deserialization with a slot with thread too high

        let slot_1 = Slot::new(1, 99);
        let index_1_0 = 0;
        let event = SCOutputEvent {
            context: EventExecutionContext {
                slot: slot_1,
                block: None,
                read_only: false,
                index_in_slot: index_1_0,
                call_stack: Default::default(),
                origin_operation_id: None,
                is_final: true,
                is_error: false,
            },
            data: "message foo bar".to_string(),
        };

        let event_ser = SCOutputEventSerializer::new();
        let event_deser = SCOutputEventDeserializer::new(SCOutputEventDeserializerArgs {
            thread_count: 16,
            max_call_stack_length: 25,
            max_event_data_length: 512,
        });

        let mut buffer = Vec::new();
        event_ser.serialize(&event, &mut buffer).unwrap();

        let res = event_deser.deserialize::<DeserializeError>(&buffer);
        // Expect deserialization to fail (slot thread too high)
        assert!(res.is_err());
    }
}
