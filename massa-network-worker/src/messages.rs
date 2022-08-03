// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::HashDeserializer;
use massa_models::{
    array_from_slice,
    constants::HANDSHAKE_RANDOMNESS_SIZE_BYTES,
    operation::OperationPrefixIds,
    operation::{
        OperationPrefixIdsDeserializer, OperationPrefixIdsSerializer, Operations,
        OperationsDeserializer, OperationsSerializer,
    },
    wrapped::{WrappedDeserializer, WrappedSerializer},
    Block, BlockDeserializer, BlockHeader, BlockHeaderDeserializer, BlockId, Endorsement,
    EndorsementDeserializer, IpAddrDeserializer, IpAddrSerializer, Version, VersionDeserializer,
    VersionSerializer, WrappedBlock, WrappedEndorsement, WrappedHeader,
};
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
    /// Whole block structure.
    Block(WrappedBlock),
    /// Block header
    BlockHeader(WrappedHeader),
    /// Message asking the peer for a block.
    AskForBlocks(Vec<BlockId>),
    /// Message asking the peer for its advertisable peers list.
    AskPeerList,
    /// Reply to a `AskPeerList` message
    /// Peers are ordered from most to less reliable.
    /// If the ip of the node that sent that message is routable,
    /// it is the first ip of the list.
    PeerList(Vec<IpAddr>),
    /// Block not found
    BlockNotFound(BlockId),
    /// Batch of operation ids
    OperationsAnnouncement(OperationPrefixIds),
    /// Someone ask for operations.
    AskForOperations(OperationPrefixIds),
    /// A list of operations
    Operations(Operations),
    /// Endorsements
    Endorsements(Vec<WrappedEndorsement>),
}

#[derive(IntoPrimitive, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u32)]
pub(crate) enum MessageTypeId {
    HandshakeInitiation = 0u32,
    HandshakeReply = 1,
    Block = 2,
    BlockHeader = 3,
    AskForBlocks = 4,
    AskPeerList = 5,
    PeerList = 6,
    BlockNotFound = 7,
    Operations = 8,
    Endorsements = 9,
    AskForOperations = 10,
    OperationsAnnouncement = 11,
}

/// Basic serializer for `Message`.
pub struct MessageSerializer {
    version_serializer: VersionSerializer,
    u32_serializer: U32VarIntSerializer,
    wrapped_serializer: WrappedSerializer,
    operation_prefix_ids_serializer: OperationPrefixIdsSerializer,
    operations_serializer: OperationsSerializer,
    ip_addr_serializer: IpAddrSerializer,
}

impl MessageSerializer {
    /// Creates a new `MessageSerializer`.
    pub fn new() -> Self {
        MessageSerializer {
            version_serializer: VersionSerializer::new(),
            u32_serializer: U32VarIntSerializer::new(),
            wrapped_serializer: WrappedSerializer::new(),
            operation_prefix_ids_serializer: OperationPrefixIdsSerializer::new(),
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
            Message::Block(block) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::Block as u32), buffer)?;
                self.wrapped_serializer.serialize(block, buffer)?;
            }
            Message::BlockHeader(header) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::BlockHeader as u32), buffer)?;
                self.wrapped_serializer.serialize(header, buffer)?;
            }
            Message::AskForBlocks(block_ids) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::AskForBlocks as u32), buffer)?;
                self.u32_serializer
                    .serialize(&(block_ids.len() as u32), buffer)?;
                for block_id in block_ids {
                    buffer.extend(block_id.0.to_bytes());
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
            Message::BlockNotFound(block_id) => {
                self.u32_serializer
                    .serialize(&(MessageTypeId::BlockNotFound as u32), buffer)?;
                buffer.extend(block_id.0.to_bytes());
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
                    self.wrapped_serializer.serialize(endorsement, buffer)?;
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
    operations_deserializer: OperationsDeserializer,
    hash_deserializer: HashDeserializer,
    block_header_deserializer: WrappedDeserializer<BlockHeader, BlockHeaderDeserializer>,
    block_deserializer: WrappedDeserializer<Block, BlockDeserializer>,
    endorsements_length_deserializer: U32VarIntDeserializer,
    endorsement_deserializer: WrappedDeserializer<Endorsement, EndorsementDeserializer>,
    operation_prefix_ids_deserializer: OperationPrefixIdsDeserializer,
    ip_addr_deserializer: IpAddrDeserializer,
}

impl MessageDeserializer {
    /// Creates a new `MessageDeserializer`.
    pub fn new(
        thread_count: u8,
        endorsement_count: u32,
        max_advertise_length: u32,
        max_ask_block: u32,
        max_operations_per_block: u32,
        max_operations_per_message: u32,
        max_endorsements_per_message: u32,
    ) -> Self {
        MessageDeserializer {
            public_key_deserializer: PublicKeyDeserializer::new(),
            signature_deserializer: SignatureDeserializer::new(),
            version_deserializer: VersionDeserializer::new(),
            id_deserializer: U32VarIntDeserializer::new(Included(0), Included(200)),
            ask_block_number_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Included(max_ask_block),
            ),
            peer_list_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(max_advertise_length),
            ),
            operations_deserializer: OperationsDeserializer::new(),
            hash_deserializer: HashDeserializer::new(),
            block_deserializer: WrappedDeserializer::new(BlockDeserializer::new(
                thread_count,
                max_operations_per_block,
                endorsement_count,
            )),
            block_header_deserializer: WrappedDeserializer::new(BlockHeaderDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            endorsements_length_deserializer: U32VarIntDeserializer::new(
                Included(0),
                Excluded(max_endorsements_per_message),
            ),
            endorsement_deserializer: WrappedDeserializer::new(EndorsementDeserializer::new(
                thread_count,
                endorsement_count,
            )),
            operation_prefix_ids_deserializer: OperationPrefixIdsDeserializer::new(
                max_operations_per_message,
            ),
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
                MessageTypeId::Block => context("Failed Block deserialization", |input| {
                    self.block_deserializer.deserialize(input)
                })
                .map(Message::Block)
                .parse(input),
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
                        context("Failed blockId deserialization", |input| {
                            self.hash_deserializer
                                .deserialize(input)
                                .map(|(rest, id)| (rest, BlockId(id)))
                        }),
                    ),
                )
                .map(Message::AskForBlocks)
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
                MessageTypeId::BlockNotFound => {
                    context("Failed BlockNotFound deserialization", |input| {
                        self.hash_deserializer.deserialize(input)
                    })
                    .map(|block_id| Message::BlockNotFound(BlockId(block_id)))
                    .parse(input)
                }
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
    use massa_models::constants::{
        ENDORSEMENT_COUNT, MAX_ADVERTISE_LENGTH, MAX_ASK_BLOCKS_PER_MESSAGE,
        MAX_ENDORSEMENTS_PER_MESSAGE, MAX_OPERATIONS_PER_BLOCK, MAX_OPERATIONS_PER_MESSAGE,
        THREAD_COUNT,
    };
    use massa_serialization::DeserializeError;
    use massa_signature::KeyPair;
    use rand::{prelude::StdRng, RngCore, SeedableRng};
    use serial_test::serial;
    use std::str::FromStr;

    fn initialize_context() -> massa_models::SerializationContext {
        // Init the serialization context with a default,
        // can be overwritten with a more specific one in the test.
        let ctx = massa_models::SerializationContext {
            max_operations_per_block: 1024,
            thread_count: 2,
            max_advertise_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_block_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
            max_operations_per_message: 1024,
            max_endorsements_per_message: 1024,
            max_bootstrap_message_size: 100000000,
            max_bootstrap_pos_entries: 1000,
            max_bootstrap_pos_cycles: 5,
            endorsement_count: 8,
        };
        massa_models::init_serialization_context(ctx.clone());
        ctx
    }

    #[test]
    #[serial]
    fn test_ser_deser() {
        initialize_context();
        let message_serializer = MessageSerializer::new();
        let message_deserializer = MessageDeserializer::new(
            THREAD_COUNT,
            ENDORSEMENT_COUNT,
            MAX_ADVERTISE_LENGTH,
            MAX_ASK_BLOCKS_PER_MESSAGE,
            MAX_OPERATIONS_PER_BLOCK,
            MAX_OPERATIONS_PER_MESSAGE,
            MAX_ENDORSEMENTS_PER_MESSAGE,
        );
        let mut random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut random_bytes);
        let keypair = KeyPair::generate();
        let msg = Message::HandshakeInitiation {
            public_key: keypair.get_public_key(),
            random_bytes,
            version: Version::from_str("TEST.1.2").unwrap(),
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
