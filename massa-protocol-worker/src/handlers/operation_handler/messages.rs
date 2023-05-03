use massa_models::operation::{
    OperationPrefixIds, OperationPrefixIdsDeserializer, OperationPrefixIdsSerializer,
    OperationsDeserializer, OperationsSerializer, SecureShareOperation,
};
use massa_serialization::{Deserializer, SerializeError, Serializer};
use nom::{
    error::{context, ContextError, ParseError},
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};

#[derive(Debug)]
pub enum OperationMessage {
    /// Batch of operation ids
    OperationsAnnouncement(OperationPrefixIds),
    /// Someone ask for operations.
    AskForOperations(OperationPrefixIds),
    /// A list of operations
    Operations(Vec<SecureShareOperation>),
}

impl OperationMessage {
    pub fn get_id(&self) -> MessageTypeId {
        match self {
            OperationMessage::OperationsAnnouncement(_) => MessageTypeId::OperationsAnnouncement,
            OperationMessage::AskForOperations(_) => MessageTypeId::AskForOperations,
            OperationMessage::Operations(_) => MessageTypeId::Operations,
        }
    }

    pub fn max_id() -> u64 {
        <MessageTypeId as Into<u64>>::into(MessageTypeId::Operations) + 1
    }
}

// DO NOT FORGET TO UPDATE MAX ID IF YOU UPDATE THERE
#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum MessageTypeId {
    OperationsAnnouncement = 0,
    AskForOperations = 1,
    Operations = 2,
}

#[derive(Default, Clone)]
pub struct OperationMessageSerializer {
    operation_prefix_ids_serializer: OperationPrefixIdsSerializer,
    operations_serializer: OperationsSerializer,
}

impl OperationMessageSerializer {
    pub fn new() -> Self {
        Self {
            operation_prefix_ids_serializer: OperationPrefixIdsSerializer::new(),
            operations_serializer: OperationsSerializer::new(),
        }
    }
}

impl Serializer<OperationMessage> for OperationMessageSerializer {
    fn serialize(
        &self,
        value: &OperationMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), SerializeError> {
        match value {
            OperationMessage::OperationsAnnouncement(operations) => {
                self.operation_prefix_ids_serializer
                    .serialize(operations, buffer)?;
            }
            OperationMessage::AskForOperations(operations) => {
                self.operation_prefix_ids_serializer
                    .serialize(operations, buffer)?;
            }
            OperationMessage::Operations(operations) => {
                self.operations_serializer.serialize(operations, buffer)?;
            }
        }
        Ok(())
    }
}

pub struct OperationMessageDeserializer {
    operation_prefix_ids_deserializer: OperationPrefixIdsDeserializer,
    operations_deserializer: OperationsDeserializer,
    message_id: u64,
}

/// Limits used in the deserialization of `OperationMessage`
pub struct OperationMessageDeserializerArgs {
    /// Maximum number of prefix ids that can be asked to propagate or sent
    pub max_operations_prefix_ids: u32,
    /// Maximum of full operations sent in one message
    pub max_operations: u32,
    //TODO: All of this arguments should be in a `OperationDeserializer` struct that would be used here
    /// Maximum size of a user datastore value
    pub max_datastore_value_length: u64,
    /// Maximum size of a function name
    pub max_function_name_length: u16,
    /// Maximum size of parameters
    pub max_parameters_size: u32,
    /// Maximum number of entries in the op datastore
    pub max_op_datastore_entry_count: u64,
    /// Maximum size of a op datastore key
    pub max_op_datastore_key_length: u8,
    /// Maximum size of a op datastore value
    pub max_op_datastore_value_length: u64,
}

impl OperationMessageDeserializer {
    pub fn new(args: OperationMessageDeserializerArgs) -> Self {
        Self {
            operation_prefix_ids_deserializer: OperationPrefixIdsDeserializer::new(
                args.max_operations_prefix_ids,
            ),
            operations_deserializer: OperationsDeserializer::new(
                args.max_operations,
                args.max_datastore_value_length,
                args.max_function_name_length,
                args.max_parameters_size,
                args.max_op_datastore_entry_count,
                args.max_op_datastore_key_length,
                args.max_op_datastore_value_length,
            ),
            message_id: 0,
        }
    }

    pub fn set_message_id(&mut self, id: u64) {
        self.message_id = id;
    }
}

impl Deserializer<OperationMessage> for OperationMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], OperationMessage, E> {
        context("Failed OperationMessage deserialization", |buffer| {
            let id = MessageTypeId::try_from(self.message_id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                MessageTypeId::AskForOperations => {
                    context("Failed AskForOperations deserialization", |input| {
                        self.operation_prefix_ids_deserializer.deserialize(input)
                    })
                    .map(OperationMessage::AskForOperations)
                    .parse(buffer)
                }
                MessageTypeId::OperationsAnnouncement => {
                    context("Failed OperationsAnnouncement deserialization", |input| {
                        self.operation_prefix_ids_deserializer.deserialize(input)
                    })
                    .map(OperationMessage::OperationsAnnouncement)
                    .parse(buffer)
                }
                MessageTypeId::Operations => {
                    context("Failed Operations deserialization", |input| {
                        self.operations_deserializer.deserialize(input)
                    })
                    .map(OperationMessage::Operations)
                    .parse(buffer)
                }
            }
        })
        .parse(buffer)
    }
}
