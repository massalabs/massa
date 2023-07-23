use massa_models::{
    block_header::{BlockHeader, BlockHeaderDeserializer, SecuredHeader},
    block_id::{BlockId, BlockIdDeserializer, BlockIdSerializer},
    operation::{
        OperationId, OperationIdSerializer, OperationIdsDeserializer, OperationsDeserializer,
        SecureShareOperation,
    },
    secure_share::{SecureShareDeserializer, SecureShareSerializer},
};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U64VarIntDeserializer, U64VarIntSerializer,
};
use nom::{
    error::{context, ContextError, ParseError},
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use std::ops::Bound::Included;

/// Request block data
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AskForBlockInfo {
    /// Ask header
    Header,
    /// Ask for the list of operation IDs of the block
    #[default]
    OperationIds,
    /// Ask for a subset of operations of the block
    Operations(Vec<OperationId>),
}

/// Reply to a block data request
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum BlockInfoReply {
    /// Header
    Header(SecuredHeader),
    /// List of operation IDs within the block
    OperationIds(Vec<OperationId>),
    /// Requested full operations of the block
    Operations(Vec<SecureShareOperation>),
    /// Block not found
    NotFound,
}

#[derive(Debug)]
//TODO: Fix this clippy warning
#[allow(clippy::large_enum_variant)]
pub enum BlockMessage {
    /// Block header
    BlockHeader(SecuredHeader),
    /// Message asking the peer for info on a list of blocks.
    BlockDataRequest {
        /// ID of the block to ask info for.
        block_id: BlockId,
        /// Block info to ask for.
        block_info: AskForBlockInfo,
    },
    /// Message replying with info on a list of blocks.
    BlockDataResponse {
        /// ID of the block to reply info for.
        block_id: BlockId,
        /// Block info reply.
        block_info: BlockInfoReply,
    },
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum MessageTypeId {
    BlockHeader,
    BlockDataRequest,
    BlockDataResponse,
}

impl From<&BlockMessage> for MessageTypeId {
    fn from(value: &BlockMessage) -> Self {
        match value {
            BlockMessage::BlockHeader(_) => MessageTypeId::BlockHeader,
            BlockMessage::BlockDataRequest { .. } => MessageTypeId::BlockDataRequest,
            BlockMessage::BlockDataResponse { .. } => MessageTypeId::BlockDataResponse,
        }
    }
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u64)]
pub enum BlockInfoType {
    Header = 0,
    OperationIds = 1,
    Operations = 2,
    NotFound = 3,
}

#[derive(Default, Clone)]
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
        self.id_serializer.serialize(
            &MessageTypeId::from(value).try_into().map_err(|_| {
                SerializeError::GeneralError(String::from("Failed to serialize id"))
            })?,
            buffer,
        )?;
        match value {
            BlockMessage::BlockHeader(header) => {
                self.secure_share_serializer.serialize(header, buffer)?;
            }
            BlockMessage::BlockDataRequest {
                block_id,
                block_info,
            } => {
                self.block_id_serializer.serialize(block_id, buffer)?;
                match block_info {
                    AskForBlockInfo::Header => {
                        self.id_serializer
                            .serialize(&(BlockInfoType::Header as u64), buffer)?;
                    }
                    AskForBlockInfo::OperationIds => {
                        self.id_serializer
                            .serialize(&(BlockInfoType::OperationIds as u64), buffer)?;
                    }
                    AskForBlockInfo::Operations(operations_ids) => {
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
            BlockMessage::BlockDataResponse {
                block_id,
                block_info,
            } => {
                self.block_id_serializer.serialize(block_id, buffer)?;
                match block_info {
                    BlockInfoReply::Header(header) => {
                        self.id_serializer
                            .serialize(&(BlockInfoType::Header as u64), buffer)?;
                        self.secure_share_serializer.serialize(header, buffer)?;
                    }
                    BlockInfoReply::OperationIds(operations_ids) => {
                        self.id_serializer
                            .serialize(&(BlockInfoType::OperationIds as u64), buffer)?;
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
        Ok(())
    }
}

pub struct BlockMessageDeserializer {
    id_deserializer: U64VarIntDeserializer,
    block_header_deserializer: SecureShareDeserializer<BlockHeader, BlockHeaderDeserializer>,
    block_id_deserializer: BlockIdDeserializer,
    operation_ids_deserializer: OperationIdsDeserializer,
    operations_deserializer: OperationsDeserializer,
}

pub struct BlockMessageDeserializerArgs {
    pub thread_count: u8,
    pub endorsement_count: u32,
    pub max_operations_per_block: u32,
    pub max_datastore_value_length: u64,
    pub max_function_name_length: u16,
    pub max_parameters_size: u32,
    pub max_op_datastore_entry_count: u64,
    pub max_op_datastore_key_length: u8,
    pub max_op_datastore_value_length: u64,
    pub max_denunciations_in_block_header: u32,
    pub last_start_period: Option<u64>,
}

impl BlockMessageDeserializer {
    pub fn new(args: BlockMessageDeserializerArgs) -> Self {
        Self {
            id_deserializer: U64VarIntDeserializer::new(Included(0), Included(u64::MAX)),
            block_header_deserializer: SecureShareDeserializer::new(BlockHeaderDeserializer::new(
                args.thread_count,
                args.endorsement_count,
                args.max_denunciations_in_block_header,
                args.last_start_period,
            )),
            block_id_deserializer: BlockIdDeserializer::new(),
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
}

impl Deserializer<BlockMessage> for BlockMessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], BlockMessage, E> {
        context("Failed BlockMessage deserialization", |buffer| {
            let (buffer, raw_id) = self.id_deserializer.deserialize(buffer)?;
            let id = MessageTypeId::try_from(raw_id).map_err(|_| {
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
                MessageTypeId::BlockDataRequest => context(
                    "Failed BlockDataRequest deserialization",
                    tuple((
                        context("Failed BlockId deserialization", |input| {
                            self.block_id_deserializer
                                .deserialize(input)
                                .map(|(rest, id)| (rest, id))
                        }),
                        context("Failed infos deserialization", |input| {
                            let (rest, raw_id) = self.id_deserializer.deserialize(input)?;
                            let info_type: BlockInfoType = raw_id.try_into().map_err(|_| {
                                nom::Err::Error(ParseError::from_error_kind(
                                    buffer,
                                    nom::error::ErrorKind::Digit,
                                ))
                            })?;
                            match info_type {
                                BlockInfoType::Header => Ok((rest, AskForBlockInfo::Header)),
                                BlockInfoType::OperationIds => {
                                    Ok((rest, AskForBlockInfo::OperationIds))
                                }
                                BlockInfoType::Operations => self
                                    .operation_ids_deserializer
                                    .deserialize(rest)
                                    .map(|(rest, operation_ids)| {
                                        (rest, AskForBlockInfo::Operations(operation_ids))
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
                )
                .map(|(block_id, block_info)| BlockMessage::BlockDataRequest {
                    block_id,
                    block_info,
                })
                .parse(buffer),
                MessageTypeId::BlockDataResponse => context(
                    "Failed BlockDataResponse deserialization",
                    tuple((
                        context("Failed BlockId deserialization", |input| {
                            self.block_id_deserializer
                                .deserialize(input)
                                .map(|(rest, id)| (rest, id))
                        }),
                        context("Failed infos deserialization", |input| {
                            let (rest, raw_id) = self.id_deserializer.deserialize(input)?;
                            let info_type: BlockInfoType = raw_id.try_into().map_err(|_| {
                                nom::Err::Error(ParseError::from_error_kind(
                                    buffer,
                                    nom::error::ErrorKind::Digit,
                                ))
                            })?;
                            match info_type {
                                BlockInfoType::Header => self
                                    .block_header_deserializer
                                    .deserialize(rest)
                                    .map(|(rest, header)| (rest, BlockInfoReply::Header(header))),
                                BlockInfoType::OperationIds => self
                                    .operation_ids_deserializer
                                    .deserialize(rest)
                                    .map(|(rest, operation_ids)| {
                                        (rest, BlockInfoReply::OperationIds(operation_ids))
                                    }),
                                BlockInfoType::Operations => self
                                    .operations_deserializer
                                    .deserialize(rest)
                                    .map(|(rest, operations)| {
                                        (rest, BlockInfoReply::Operations(operations))
                                    }),
                                BlockInfoType::NotFound => Ok((rest, BlockInfoReply::NotFound)),
                            }
                        }),
                    )),
                )
                .map(|(block_id, block_info)| BlockMessage::BlockDataResponse {
                    block_id,
                    block_info,
                })
                .parse(buffer),
            }
        })
        .parse(buffer)
    }
}
