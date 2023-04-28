// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::HashDeserializer;
use massa_models::{
    block_header::{BlockHeader, BlockHeaderDeserializer, SecuredHeader},
    block_id::BlockId,
    config::HANDSHAKE_RANDOMNESS_SIZE_BYTES,
    endorsement::{Endorsement, EndorsementDeserializer, SecureShareEndorsement},
    operation::{
        OperationIdsDeserializer, OperationIdsSerializer, OperationPrefixIds,
        OperationPrefixIdsDeserializer, OperationPrefixIdsSerializer, OperationsDeserializer,
        OperationsSerializer, SecureShareOperation,
    },
    secure_share::{SecureShareDeserializer, SecureShareSerializer},
    serialization::array_from_slice,
    serialization::{IpAddrDeserializer, IpAddrSerializer},
    version::{Version, VersionDeserializer, VersionSerializer},
};
use massa_network_exports::{AskForBlocksInfo, BlockInfoReply};
use massa_serialization::{
    Deserializer, SerializeError, Serializer, U32VarIntDeserializer, U32VarIntSerializer,
};
use massa_signature::{PublicKey, PublicKeyDeserializer, Signature, SignatureDeserializer};
use nom::{
    bytes::complete::take,
    error::{context, ContextError, ParseError},
    multi::length_count,
    sequence::tuple,
    IResult, Parser,
};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::ops::Bound::{Excluded, Included};

/// All messages that can be sent or received.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// Initiates handshake.
    HandshakeInitiation {
        /// Our `public_key`, so the peer can decode our reply.
        public_key: PublicKey,
        /// Random data we expect the peer to sign with its `keypair`.
        /// They should send us their handshake initiation message to
        /// let us know their public key.
        random_bytes: [u8; HANDSHAKE_RANDOMNESS_SIZE_BYTES],
        version: Version,
    },
    /// Reply to a handshake initiation message.
    HandshakeReply {
        /// Signature of the received random bytes with our `keypair`.
        signature: Signature,
    },
    /// Block header
    BlockHeader(SecuredHeader),
    /// Message asking the peer for info on a list of blocks.
    AskForBlocks(Vec<(BlockId, AskForBlocksInfo)>),
    /// Message replying with info on a list of blocks.
    ReplyForBlocks(Vec<(BlockId, BlockInfoReply)>),
    /// Message asking the peer for its advertisable peers list.
    AskPeerList,
    /// Reply to a `AskPeerList` message
    /// Peers are ordered from most to less reliable.
    /// If the ip of the node that sent that message is routable,
    /// it is the first ip of the list.
    PeerList(Vec<IpAddr>),
    /// Batch of operation ids
    OperationsAnnouncement(OperationPrefixIds),
    /// Someone ask for operations.
    AskForOperations(OperationPrefixIds),
    /// A list of operations
    Operations(Vec<SecureShareOperation>),
    /// Endorsements
    Endorsements(Vec<SecureShareEndorsement>),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
pub(crate) enum MessageTypeId {
    HandshakeInitiation = 0u32,
    HandshakeReply,
    BlockHeader,
    AskForBlocks,
    AskPeerList,
    PeerList,
    Operations,
    Endorsements,
    AskForOperations,
    OperationsAnnouncement,
    ReplyForBlocks,
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
pub(crate) enum BlockInfoType {
    Header = 0u32,
    Info,
    Operations,
    NotFound,
}

/// Basic serializer for `Message`.
pub struct MessageSerializer {
    version_serializer: VersionSerializer,
    u32_serializer: U32VarIntSerializer,
    secure_serializer: SecureShareSerializer,
    operation_prefix_ids_serializer: OperationPrefixIdsSerializer,
    operations_ids_serializer: OperationIdsSerializer,
    operations_serializer: OperationsSerializer,
    ip_addr_serializer: IpAddrSerializer,
}

impl MessageSerializer {
    /// Creates a new `MessageSerializer`.
    pub fn new() -> Self {
        MessageSerializer {
            version_serializer: VersionSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
            secure_serializer: SecureShareSerializer::new(),
            operation_prefix_ids_serializer: OperationPrefixIdsSerializer::new(),
            operations_ids_serializer: OperationIdsSerializer::new(),
            operations_serializer: OperationsSerializer::new(),
            ip_addr_serializer: IpAddrSerializer::new(),
        }
    }
}

impl Default for MessageSerializer {
    fn default() -> Self {
        Self::new()
    }
}

/// For more details on how incoming objects are checked for validity at this stage,
/// see their serializer in `massa-models`.
impl Serializer<Message> for MessageSerializer {
    fn serialize(&self, value: &Message, buffer: &mut Vec<u8>) -> Result<(), SerializeError> {
        match &value {
            Message::HandshakeInitiation {
                public_key,
                random_bytes,
                version,
            } => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::HandshakeInitiation as u32), buffer)?;
                buffer.extend(public_key.to_bytes());
                buffer.extend(random_bytes);
                self.version_serializer.serialize(version, buffer)?;
            }
            Message::HandshakeReply { signature } => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::HandshakeReply as u32), buffer)?;
                buffer.extend(signature.to_bytes());
            }
            Message::BlockHeader(header) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::BlockHeader as u32), buffer)?;
                self.secure_serializer.serialize(header, buffer)?;
            }
            Message::AskForBlocks(list) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::AskForBlocks as u32), buffer)?;
                self.u32_serializer
                    .serialize(&(list.len() as u32), buffer)?;
                for (hash, info) in list {
                    buffer.extend(hash.to_bytes());
                    let info_type = match info {
                        AskForBlocksInfo::Header => BlockInfoType::Header,
                        AskForBlocksInfo::Info => BlockInfoType::Info,
                        AskForBlocksInfo::Operations(_) => BlockInfoType::Operations,
                    };
                    self.u32_serializer
                        .serialize(&u32::from(info_type), buffer)?;
                    if let AskForBlocksInfo::Operations(ids) = info {
                        self.operations_ids_serializer.serialize(ids, buffer)?;
                    }
                }
            }
            Message::ReplyForBlocks(list) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::ReplyForBlocks as u32), buffer)?;
                self.u32_serializer
                    .serialize(&(list.len() as u32), buffer)?;
                for (hash, info) in list {
                    buffer.extend(hash.to_bytes());
                    let info_type = match info {
                        BlockInfoReply::Header(_) => BlockInfoType::Header,
                        BlockInfoReply::Info(_) => BlockInfoType::Info,
                        BlockInfoReply::Operations(_) => BlockInfoType::Operations,
                        BlockInfoReply::NotFound => BlockInfoType::NotFound,
                    };
                    self.u32_serializer
                        .serialize(&u32::from(info_type), buffer)?;
                    if let BlockInfoReply::Header(header) = info {
                        self.secure_serializer.serialize(header, buffer)?;
                    }
                    if let BlockInfoReply::Operations(ops) = info {
                        self.operations_serializer.serialize(ops, buffer)?;
                    }
                    if let BlockInfoReply::Info(ids) = info {
                        self.operations_ids_serializer.serialize(ids, buffer)?;
                    }
                }
            }
            Message::AskPeerList => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::AskPeerList as u32), buffer)?;
            }
            Message::PeerList(peers) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::PeerList as u32), buffer)?;
                self.u32_serializer
                    .serialize(&(peers.len() as u32), buffer)?;
                for peer in peers {
                    self.ip_addr_serializer.serialize(peer, buffer)?;
                }
            }
            Message::OperationsAnnouncement(operation_prefix_ids) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::OperationsAnnouncement as u32), buffer)?;
                self.operation_prefix_ids_serializer
                    .serialize(operation_prefix_ids, buffer)?;
            }
            Message::AskForOperations(operation_prefix_ids) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::AskForOperations as u32), buffer)?;
                self.operation_prefix_ids_serializer
                    .serialize(operation_prefix_ids, buffer)?;
            }
            Message::Operations(operations) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::Operations as u32), buffer)?;
                self.operations_serializer.serialize(operations, buffer)?;
            }
            Message::Endorsements(endorsements) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::Endorsements as u32), buffer)?;
                self.u32_serializer
                    .serialize(&(endorsements.len() as u32), buffer)?;
                for endorsement in endorsements {
                    self.secure_serializer.serialize(endorsement, buffer)?;
                }
            }
        }
        Ok(())
    }
}

/// Basic deserializer for `Message`.
pub struct MessageDeserializer {
    public_key_deserializer: PublicKeyDeserializer,
    signature_deserializer: SignatureDeserializer,
    version_deserializer: VersionDeserializer,
    id_deserializer: U32VarIntDeserializer,
    ask_block_number_deserializer: U32VarIntDeserializer,
    peer_list_length_deserializer: U32VarIntDeserializer,
    operation_replies_deserializer: OperationsDeserializer,
    operations_deserializer: OperationsDeserializer,
    hash_deserializer: HashDeserializer,
    block_header_deserializer: SecureShareDeserializer<BlockHeader, BlockHeaderDeserializer>,
    endorsements_length_deserializer: U32VarIntDeserializer,
    endorsement_deserializer: SecureShareDeserializer<Endorsement, EndorsementDeserializer>,
    operation_prefix_ids_deserializer: OperationPrefixIdsDeserializer,
    infos_deserializer: OperationIdsDeserializer,
    ip_addr_deserializer: IpAddrDeserializer,
}

impl MessageDeserializer {
    /// Creates a new `MessageDeserializer`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_advertise_length: u32,
        max_ask_block: u32,
        max_operations_per_block: u32,
        max_operations_per_message: u32,
        max_endorsements_per_message: u32,
        max_datastore_value_length: u64,
        max_function_name_length: u16,
        max_parameters_size: u32,
        max_op_datastore_entry_count: u64,
        max_op_datastore_key_length: u8,
        max_op_datastore_value_length: u64,
        max_denunciations_per_block_header: u32,
        last_start_period: Option<u64>,
    ) -> Self {
        MessageDeserializer {
            public_key_deserializer: PublicKeyDeserializer::new(),
            signature_deserializer: SignatureDeserializer::new(),
            version_deserializer: VersionDeserializer::new(),
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(u32::MAX)),
            ask_block_number_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_ask_block),
            ),
            peer_list_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_advertise_length),
            ),
            operation_replies_deserializer: OperationsDeserializer::new(
                max_operations_per_block,
                max_datastore_value_length,
                max_function_name_length,
                max_parameters_size,
                max_op_datastore_entry_count,
                max_op_datastore_key_length,
                max_op_datastore_value_length,
            ),
            operations_deserializer: OperationsDeserializer::new(
                max_operations_per_message,
                max_datastore_value_length,
                max_function_name_length,
                max_parameters_size,
                max_op_datastore_entry_count,
                max_op_datastore_key_length,
                max_op_datastore_value_length,
            ),
            hash_deserializer: HashDeserializer::new(),
            block_header_deserializer: SecureShareDeserializer::new(BlockHeaderDeserializer::new(
                thread_count,
                endorsement_count,
                max_denunciations_per_block_header,
                last_start_period,
            )),
            endorsements_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(max_endorsements_per_message),
            ),
            endorsement_deserializer: SecureShareDeserializer::new(EndorsementDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            operation_prefix_ids_deserializer: OperationPrefixIdsDeserializer::new(
                max_operations_per_message,
            ),
            infos_deserializer: OperationIdsDeserializer::new(max_operations_per_block),
            ip_addr_deserializer: IpAddrDeserializer::new(),
        }
    }
}

impl Deserializer<Message> for MessageDeserializer {
    fn deserialize<'a, E: ParseError<&'a [u8]> + ContextError<&'a [u8]>>(
        &self,
        buffer: &'a [u8],
    ) -> IResult<&'a [u8], Message, E> {
        context("Failed Message deserialization", |buffer| {
            let (input, id) = self.id_deserializer.deserialize(buffer)?;
            let id = MessageTypeId::try_from(id).map_err(|_| {
                nom::Err::Error(ParseError::from_error_kind(
                    buffer,
                    nom::error::ErrorKind::Eof,
                ))
            })?;
            match id {
                MessageTypeId::HandshakeInitiation => context(
                    "Failed HandshakeInitiation deserialization",
                    tuple((
                        context("Failed public_key deserialization", |input| {
                            self.public_key_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed random_bytes deserialization",
                            take(HANDSHAKE_RANDOMNESS_SIZE_BYTES),
                        ),
                        context("Failed version deserialization", |input| {
                            self.version_deserializer.deserialize(input)
                        }),
                    ))
                    .map(|(public_key, random_bytes, version)| {
                        // Unwrap safety: we checked above that we took enough bytes
                        Message::HandshakeInitiation {
                            public_key,
                            random_bytes: array_from_slice(random_bytes).unwrap(),
                            version,
                        }
                    }),
                )
                .parse(input),
                MessageTypeId::HandshakeReply => {
                    context("Failed HandshakeReply deserialization", |input| {
                        self.signature_deserializer.deserialize(input)
                    })
                    .map(|signature| Message::HandshakeReply { signature })
                    .parse(input)
                }
                MessageTypeId::BlockHeader => {
                    context("Failed BlockHeader deserialization", |input| {
                        self.block_header_deserializer.deserialize(input)
                    })
                    .map(Message::BlockHeader)
                    .parse(input)
                }
                MessageTypeId::AskForBlocks => context(
                    "Failed AskForBlocks deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.ask_block_number_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed blockId deserialization",
                            tuple((
                                |input| {
                                    self.hash_deserializer
                                        .deserialize(input)
                                        .map(|(rest, id)| (rest, BlockId(id)))
                                },
                                |input| {
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
                                            .infos_deserializer
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
                                },
                            )),
                        ),
                    ),
                )
                .map(Message::AskForBlocks)
                .parse(input),
                MessageTypeId::ReplyForBlocks => context(
                    "Failed ReplyForBlocks deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.ask_block_number_deserializer.deserialize(input)
                        }),
                        context(
                            "Failed blockId deserialization",
                            tuple((
                                |input| {
                                    self.hash_deserializer
                                        .deserialize(input)
                                        .map(|(rest, id)| (rest, BlockId(id)))
                                },
                                |input| {
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
                                            .infos_deserializer
                                            .deserialize(rest)
                                            .map(|(rest, operation_ids)| {
                                                (rest, BlockInfoReply::Info(operation_ids))
                                            }),
                                        BlockInfoType::Operations => self
                                            .operation_replies_deserializer
                                            .deserialize(rest)
                                            .map(|(rest, operations)| {
                                                (rest, BlockInfoReply::Operations(operations))
                                            }),
                                        BlockInfoType::NotFound => {
                                            Ok((rest, BlockInfoReply::NotFound))
                                        }
                                    }
                                },
                            )),
                        ),
                    ),
                )
                .map(Message::ReplyForBlocks)
                .parse(input),
                MessageTypeId::AskPeerList => Ok((input, Message::AskPeerList)),
                MessageTypeId::PeerList => context(
                    "Failed PeerList deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.peer_list_length_deserializer.deserialize(input)
                        }),
                        context("Failed peer deserialization", |input| {
                            self.ip_addr_deserializer.deserialize(input)
                        }),
                    ),
                )
                .map(Message::PeerList)
                .parse(input),
                MessageTypeId::Operations => {
                    context("Failed Operations deserialization", |input| {
                        self.operations_deserializer.deserialize(input)
                    })
                    .map(Message::Operations)
                    .parse(input)
                }
                MessageTypeId::AskForOperations => {
                    context("Failed AskForOperations deserialization", |input| {
                        self.operation_prefix_ids_deserializer.deserialize(input)
                    })
                    .map(Message::AskForOperations)
                    .parse(input)
                }
                MessageTypeId::OperationsAnnouncement => {
                    context("Failed OperationsAnnouncement deserialization", |input| {
                        self.operation_prefix_ids_deserializer.deserialize(input)
                    })
                    .map(Message::OperationsAnnouncement)
                    .parse(input)
                }
                MessageTypeId::Endorsements => context(
                    "Failed Endorsements deserialization",
                    length_count(
                        context("Failed length deserialization", |input| {
                            self.endorsements_length_deserializer.deserialize(input)
                        }),
                        context("Failed endorsement deserialization", |input| {
                            self.endorsement_deserializer.deserialize(input)
                        }),
                    ),
                )
                .map(Message::Endorsements)
                .parse(input),
            }
        })
        .parse(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use massa_models::config::{
        ENDORSEMENT_COUNT, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
        MAX_DATASTORE_VALUE_LENGTH, MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
        MAX_ENDORSEMENTS_PER_MESSAGE, MAX_FUNCTION_NAME_LENGTH, MAX_OPERATIONS_PER_BLOCK,
        MAX_OPERATIONS_PER_MESSAGE, MAX_OPERATION_DATASTORE_ENTRY_COUNT,
        MAX_OPERATION_DATASTORE_KEY_LENGTH, MAX_OPERATION_DATASTORE_VALUE_LENGTH,
        MAX_PARAMETERS_SIZE, THREAD_COUNT,
    };
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use rand::{prelude::StdRng, RngCore, SeedableRng};
    use serial_test::serial;
    use std::str::FromStr;

    #[test]
    #[serial]
    fn test_ser_deser() {
        let message_serializer = MessageSerializer::new();
        let message_deserializer = MessageDeserializer::new(
            THREAD_COUNT,
            ENDORSEMENT_COUNT,
            MAX_ADVERTISE_LENGTH,
            MAX_ASK_BLOCKS_PER_MESSAGE,
            MAX_OPERATIONS_PER_BLOCK,
            MAX_OPERATIONS_PER_MESSAGE,
            MAX_ENDORSEMENTS_PER_MESSAGE,
            MAX_DATASTORE_VALUE_LENGTH,
            MAX_FUNCTION_NAME_LENGTH,
            MAX_PARAMETERS_SIZE,
            MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            MAX_OPERATION_DATASTORE_KEY_LENGTH,
            MAX_OPERATION_DATASTORE_VALUE_LENGTH,
            MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            Some(0),
        );
        let mut random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut random_bytes);
        let keypair = KeyPair::generate(0).unwrap();
        let msg = Message::HandshakeInitiation {
            public_key: keypair.get_public_key(),
            random_bytes,
            version: Version::from_str("TEST.1.10").unwrap(),
        };
        let mut ser = Vec::new();
        message_serializer.serialize(&msg, &mut ser).unwrap();
        let (_, deser) = message_deserializer
            .deserialize::<DeserializeError>(&ser)
            .unwrap();
        match (msg, deser) {
            (
                Message::HandshakeInitiation {
                    public_key: pk1,
                    random_bytes: rb1,
                    version: v1,
                },
                Message::HandshakeInitiation {
                    public_key,
                    random_bytes,
                    version,
                },
            ) => {
                assert_eq!(pk1, public_key);
                assert_eq!(rb1, random_bytes);
                assert_eq!(v1, version);
            }
            _ => panic!("unexpected message"),
        }
    }
}
