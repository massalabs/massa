use massa_hash::HashDeserializer;
use massa_models::{
    block_header::{BlockHeader, BlockHeaderDeserializer, SecuredHeader},
    block_id::{BlockId, BlockIdSerializer},
    operation::{
        OperationId, OperationIdSerializer, OperationIdsDeserializer, OperationsDeserializer,
        SecureShareOperation,
    },
    secure_share::{SecureShareDeserializer, SecureShareSerializer},
};
use massa_serialization::{Deserializer, Serializer, U64VarIntDeserializer, U64VarIntSerializer};
use nom::{
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

/// Ask for the info about a block.
#[derive(Debug, Clone, Default)]
pub enum AskForBlocksInfo {
    /// Ask header
    Header,
    /// The info about the block is required(list of operations ids).
    #[default]
    Info,
    /// The actual operations are required.
    Operations(Vec<OperationId>),
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BlockInfoReply {
    /// Header
    Header(SecuredHeader),
    /// The info about the block is required(list of operations ids).
    Info(Vec<OperationId>),
    /// The actual operations required.
    Operations(Vec<SecureShareOperation>),
    /// Block not found
    NotFound,
}

#[derive(Debug)]
pub enum BlockMessage {
    /// Block header
    BlockHeader(SecuredHeader),
    /// Message asking the peer for info on a list of blocks.
    AskForBlocks(Vec<(BlockId, AskForBlocksInfo)>),
    /// Message replying with info on a list of blocks.
    ReplyForBlocks(Vec<(BlockId, BlockInfoReply)>),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
enum MessageTypeId {
    BlockHeader,
    AskForBlocks,
    ReplyForBlocks,
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum BlockInfoType {
    Header = 0,
    Info = 1,
    Operations = 2,
    NotFound = 3,
}

#[derive(Default)]
pub struct BlockMessageSerializer {
    id_serializer: U64VarIntSerializer,
    secure_share_serializer: SecureShareSerializer,
    length_serializer: U64VarIntSerializer,
    block_id_serializer: BlockIdSerializer,
    operation_id_serializer: OperationIdSerializer,
}

impl BlockMessageSerializer {
    pub fn new() -> Self {
        Self {
            id_serializer: U64VarIntSerializer::new(),
            secure_share_serializer: SecureShareSerializer::new(),
            length_serializer: U64VarIntSerializer::new(),
            block_id_serializer: BlockIdSerializer::new(),
            operation_id_serializer: OperationIdSerializer::new(),
        }
    }
}

impl Serializer<BlockMessage> for BlockMessageSerializer {
    fn serialize(
        &self,
        value: &BlockMessage,
        buffer: &mut Vec<u8>,
    ) -> Result<(), massa_serialization::SerializeError> {
        match value {
            BlockMessage::BlockHeader(endorsements) => {
                self.id_serializer
                    .serialize(&(MessageTypeId::BlockHeader as u64), buffer)?;
                self.secure_share_serializer
                    .serialize(endorsements, buffer)?;
            }
            BlockMessage::AskForBlocks(ask_for_blocks) => {
                self.id_serializer
                    .serialize(&(MessageTypeId::AskForBlocks as u64), buffer)?;
                self.length_serializer
                    .serialize(&(ask_for_blocks.len() as u64), buffer)?;
                for (block_id, ask_for_block_info) in ask_for_blocks {
                    self.block_id_serializer.serialize(block_id, buffer)?;
                    match ask_for_block_info {
                        AskForBlocksInfo::Header => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::Header as u64), buffer)?;
                        }
                        AskForBlocksInfo::Info => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::Info as u64), buffer)?;
                        }
                        AskForBlocksInfo::Operations(operations_ids) => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::Operations as u64), buffer)?;
                            self.length_serializer
                                .serialize(&(operations_ids.len() as u64), buffer)?;
                            for operation_id in operations_ids {
                                self.operation_id_serializer
                                    .serialize(operation_id, buffer)?;
                            }
                        }
                    }
                }
            }
            BlockMessage::ReplyForBlocks(reply_for_blocks) => {
                self.id_serializer
                    .serialize(&(MessageTypeId::ReplyForBlocks as u64), buffer)?;
                self.length_serializer
                    .serialize(&(reply_for_blocks.len() as u64), buffer)?;
                for (block_id, reply_for_block_info) in reply_for_blocks {
                    self.block_id_serializer.serialize(block_id, buffer)?;
                    match reply_for_block_info {
                        BlockInfoReply::Header(header) => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::Header as u64), buffer)?;
                            self.secure_share_serializer.serialize(header, buffer)?;
                        }
                        BlockInfoReply::Info(operations_ids) => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::Info as u64), buffer)?;
                            self.length_serializer
                                .serialize(&(operations_ids.len() as u64), buffer)?;
                            for operation_id in operations_ids {
                                self.operation_id_serializer
                                    .serialize(operation_id, buffer)?;
                            }
                        }
                        BlockInfoReply::Operations(operations) => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::Operations as u64), buffer)?;
                            self.length_serializer
                                .serialize(&(operations.len() as u64), buffer)?;
                            for operation in operations {
                                self.secure_share_serializer.serialize(operation, buffer)?;
                            }
                        }
                        BlockInfoReply::NotFound => {
                            self.id_serializer
                                .serialize(&(BlockInfoType::NotFound as u64), buffer)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct BlockMessageDeserializer {
    message_id: u64,
    id_deserializer: U64VarIntDeserializer,
    block_header_deserializer: SecureShareDeserializer<BlockHeader, BlockHeaderDeserializer>,
    block_infos_length_deserializer: U64VarIntDeserializer,
    hash_deserializer: HashDeserializer,
    operation_ids_deserializer: OperationIdsDeserializer,
    operations_deserializer: OperationsDeserializer,
}

pub struct BlockMessageDeserializerArgs {
    pub thread_count: u8,
    pub endorsement_count: u32,
    pub block_infos_length_max: u64,
    pub max_operations_per_block: u32,
    pub max_datastore_value_length: u64,
    pub max_function_name_length: u16,
    pub max_parameters_size: u32,
    pub max_op_datastore_entry_count: u64,
    pub max_op_datastore_key_length: u8,
    pub max_op_datastore_value_length: u64,
}

impl BlockMessageDeserializer {
    pub fn new(args: BlockMessageDeserializerArgs) -> Self {
        Self {
            message_id: 0,
            id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            block_header_deserializer: SecureShareDeserializer::new(BlockHeaderDeserializer::new(
                args.thread_count,
                args.endorsement_count,
            )),
            block_infos_length_deserializer: U64VarIntDeserializer::new(
                Included(0),
                Included(args.block_infos_length_max),
            ),
            hash_deserializer: HashDeserializer::new(),
            operation_ids_deserializer: OperationIdsDeserializer::new(
                args.max_operations_per_block,
            ),
            operations_deserializer: OperationsDeserializer::new(
                args.max_operations_per_block,
                args.max_datastore_value_length,
                args.max_function_name_length,
                args.max_parameters_size,
                args.max_op_datastore_entry_count,
                args.max_op_datastore_key_length,
                args.max_op_datastore_value_length,
            ),
        }
    }

    pub fn set_message_id(&mut self, message_id: u64) {
        self.message_id = message_id;
    }
}

impl Deserializer<BlockMessage> for BlockMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockMessage, E> {
        context("Failed BlockMessage deserialization", |buffer| {
            let id = MessageTypeId::try_from(self.message_id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                MessageTypeId::BlockHeader => {
                    context("Failed BlockHeader deserialization", |input| {
                        self.block_header_deserializer.deserialize(input)
                    })
                    .map(BlockMessage::BlockHeader)
                    .parse(buffer)
                }
                MessageTypeId::AskForBlocks => context(
                    "Failed AskForBlocks deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.block_infos_length_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed Block infos deserialization",
                            tuple((
                                context("Failed BlockId deserialization", |input| {
                                    self.hash_deserializer
                                        .deserialize(input)
                                        .map(|(rest, id)| (rest, BlockId(id)))
                                }),
                                context("Failed infos deserialization", |input| {
                                    let (rest, raw_id) = self.id_deserializer.deserialize(input)?;
                                    let info_type: BlockInfoType =
                                        raw_id.try_into().map_err(|_| {
                                            nom::Err::Error(ParseError::from_error_kind(
                                                buffer,
                                                nom::error::ErrorKind::Digit,
                                            ))
                                        })?;
                                    match info_type {
                                        BlockInfoType::Header => {
                                            Ok((rest, AskForBlocksInfo::Header))
                                        }
                                        BlockInfoType::Info => Ok((rest, AskForBlocksInfo::Info)),
                                        BlockInfoType::Operations => self
                                            .operation_ids_deserializer
                                            .deserialize(rest)
                                            .map(|(rest, operation_ids)| {
                                                (rest, AskForBlocksInfo::Operations(operation_ids))
                                            }),
                                        BlockInfoType::NotFound => {
                                            Err(nom::Err::Error(ParseError::from_error_kind(
                                                buffer,
                                                nom::error::ErrorKind::Digit,
                                            )))
                                        }
                                    }
                                }),
                            )),
                        ),
                    ),
                )
                .map(BlockMessage::AskForBlocks)
                .parse(buffer),
                MessageTypeId::ReplyForBlocks => context(
                    "Failed ReplyForBlocks deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.block_infos_length_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed block infos deserialization",
                            tuple((
                                context("Failed BlockId deserialization", |input| {
                                    self.hash_deserializer
                                        .deserialize(input)
                                        .map(|(rest, id)| (rest, BlockId(id)))
                                }),
                                context("Failed infos deserialization", |input| {
                                    let (rest, raw_id) = self.id_deserializer.deserialize(input)?;
                                    let info_type: BlockInfoType =
                                        raw_id.try_into().map_err(|_| {
                                            nom::Err::Error(ParseError::from_error_kind(
                                                buffer,
                                                nom::error::ErrorKind::Digit,
                                            ))
                                        })?;
                                    match info_type {
                                        BlockInfoType::Header => self
                                            .block_header_deserializer
                                            .deserialize(rest)
                                            .map(|(rest, header)| {
                                                (rest, BlockInfoReply::Header(header))
                                            }),
                                        BlockInfoType::Info => self
                                            .operation_ids_deserializer
                                            .deserialize(rest)
                                            .map(|(rest, operation_ids)| {
                                                (rest, BlockInfoReply::Info(operation_ids))
                                            }),
                                        BlockInfoType::Operations => self
                                            .operations_deserializer
                                            .deserialize(rest)
                                            .map(|(rest, operations)| {
                                                (rest, BlockInfoReply::Operations(operations))
                                            }),
                                        BlockInfoType::NotFound => {
                                            Ok((rest, BlockInfoReply::NotFound))
                                        }
                                    }
                                }),
                            )),
                        ),
                    ),
                )
                .map(BlockMessage::ReplyForBlocks)
                .parse(buffer),
            }
        })
        .parse(buffer)
    }
}
