// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_models::{
    array_from_slice,
    constants::{BLOCK_ID_SIZE_BYTES, HANDSHAKE_RANDOMNESS_SIZE_BYTES},
    error::ModelsResult,
    operation::OperationPrefixIds,
    operation::{
        OperationPrefixIdsDeserializer, OperationPrefixIdsSerializer, Operations,
        OperationsDeserializer, OperationsSerializer,
    },
    with_serialization_context,
    wrapped::{WrappedDeserializer, WrappedSerializer},
    Block, BlockDeserializer, BlockHeader, BlockHeaderDeserializer, BlockId, DeserializeCompact,
    DeserializeVarInt, Endorsement, EndorsementDeserializer, IpAddrDeserializer, IpAddrSerializer,
    ModelsError, SerializeCompact, SerializeVarInt, Version, VersionDeserializer,
    VersionSerializer, WrappedBlock, WrappedEndorsement, WrappedHeader,
};
use massa_serialization::{DeserializeError, Deserializer, Serializer};
use massa_signature::{PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::{convert::TryInto, net::IpAddr};

static IP_SERIALIZER: IpAddrSerializer = IpAddrSerializer::new();

static IP_DESERIALIZER: IpAddrDeserializer = IpAddrDeserializer::new();

static OPERATION_PREFIX_ID_DESERIALIZER: OperationPrefixIdsDeserializer =
    OperationPrefixIdsDeserializer::new();

static OPERATION_PREFIX_ID_SERIALIZER: OperationPrefixIdsSerializer =
    OperationPrefixIdsSerializer::new();

static OPERATIONS_DESERIALIZER: OperationsDeserializer = OperationsDeserializer::new();

static OPERATIONS_SERIALIZER: OperationsSerializer = OperationsSerializer::new();

static VERSION_DESERIALIZER: VersionDeserializer = VersionDeserializer::new();

static VERSION_SERIALIZER: VersionSerializer = VersionSerializer::new();

static WRAPPED_BLOCK_DESERIALIZER: WrappedDeserializer<Block, BlockDeserializer> =
    WrappedDeserializer::new(BlockDeserializer::new());

static WRAPPED_BLOCK_HEADER_DESERIALIZER: WrappedDeserializer<
    BlockHeader,
    BlockHeaderDeserializer,
> = WrappedDeserializer::new(BlockHeaderDeserializer::new());

static WRAPPED_SERIALIZER: WrappedSerializer = WrappedSerializer::new();

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

/// For more details on how incoming objects are checked for validity at this stage,
/// see their implementation of `to_bytes_compact` in `models`.
impl SerializeCompact for Message {
    fn to_bytes_compact(&self) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            Message::HandshakeInitiation {
                public_key,
                random_bytes,
                version,
            } => {
                res.extend(u32::from(MessageTypeId::HandshakeInitiation).to_varint_bytes());
                res.extend(public_key.to_bytes());
                res.extend(random_bytes);
                VERSION_SERIALIZER.serialize(version, &mut res)?;
            }
            Message::HandshakeReply { signature } => {
                res.extend(u32::from(MessageTypeId::HandshakeReply).to_varint_bytes());
                res.extend(signature.to_bytes());
            }
            Message::Block(block) => {
                res.extend(u32::from(MessageTypeId::Block).to_varint_bytes());
                WRAPPED_SERIALIZER.serialize(block, &mut res)?;
            }
            Message::BlockHeader(header) => {
                res.extend(u32::from(MessageTypeId::BlockHeader).to_varint_bytes());
                WRAPPED_SERIALIZER.serialize(header, &mut res)?;
            }
            Message::AskForBlocks(list) => {
                res.extend(u32::from(MessageTypeId::AskForBlocks).to_varint_bytes());
                let list_len: u32 = list.len().try_into().map_err(|_| {
                    ModelsError::SerializeError(
                        "could not encode AskForBlocks list length as u32".into(),
                    )
                })?;
                res.extend(list_len.to_varint_bytes());
                for hash in list {
                    res.extend(hash.to_bytes());
                }
            }
            Message::AskPeerList => {
                res.extend(u32::from(MessageTypeId::AskPeerList).to_varint_bytes());
            }
            Message::PeerList(ip_vec) => {
                res.extend(u32::from(MessageTypeId::PeerList).to_varint_bytes());
                res.extend((ip_vec.len() as u64).to_varint_bytes());
                for ip in ip_vec {
                    IP_SERIALIZER.serialize(ip, &mut res)?
                }
            }
            Message::BlockNotFound(hash) => {
                res.extend(u32::from(MessageTypeId::BlockNotFound).to_varint_bytes());
                res.extend(hash.to_bytes());
            }
            Message::AskForOperations(operation_ids) => {
                res.extend(u32::from(MessageTypeId::AskForOperations).to_varint_bytes());
                OPERATION_PREFIX_ID_SERIALIZER.serialize(operation_ids, &mut res)?;
            }
            Message::OperationsAnnouncement(operation_ids) => {
                res.extend(u32::from(MessageTypeId::OperationsAnnouncement).to_varint_bytes());
                OPERATION_PREFIX_ID_SERIALIZER.serialize(operation_ids, &mut res)?;
            }
            Message::Operations(operations) => {
                res.extend(u32::from(MessageTypeId::Operations).to_varint_bytes());
                OPERATIONS_SERIALIZER.serialize(operations, &mut res)?;
            }
            Message::Endorsements(endorsements) => {
                res.extend(u32::from(MessageTypeId::Endorsements).to_varint_bytes());
                res.extend((endorsements.len() as u32).to_varint_bytes());
                for endorsement in endorsements.iter() {
                    WRAPPED_SERIALIZER.serialize(endorsement, &mut res)?;
                }
            }
        }
        Ok(res)
    }
}

/// For more details on how incoming objects are checked for validity at this stage,
/// see their implementation of `from_bytes_compact` in `models`.
impl DeserializeCompact for Message {
    fn from_bytes_compact(buffer: &[u8]) -> ModelsResult<(Self, usize)> {
        let mut cursor = 0usize;

        let (max_ask_blocks_per_message, max_peer_list_length, max_endorsements_per_message) =
            with_serialization_context(|context| {
                (
                    context.max_ask_blocks_per_message,
                    context.max_advertise_length,
                    context.max_endorsements_per_message,
                )
            });

        let (type_id_raw, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let type_id: MessageTypeId = type_id_raw
            .try_into()
            .map_err(|_| ModelsError::DeserializeError("invalid message type ID".into()))?;

        let res = match type_id {
            MessageTypeId::HandshakeInitiation => {
                // public key
                let public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += PUBLIC_KEY_SIZE_BYTES;
                // random bytes
                let random_bytes: [u8; HANDSHAKE_RANDOMNESS_SIZE_BYTES] =
                    array_from_slice(&buffer[cursor..])?;
                cursor += HANDSHAKE_RANDOMNESS_SIZE_BYTES;

                // version
                let (rest, version) = VERSION_DESERIALIZER.deserialize(&buffer[cursor..])?;
                cursor += buffer[cursor..].len() - rest.len();

                // return message
                Message::HandshakeInitiation {
                    public_key,
                    random_bytes,
                    version,
                }
            }
            MessageTypeId::HandshakeReply => {
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += SIGNATURE_SIZE_BYTES;
                Message::HandshakeReply { signature }
            }
            MessageTypeId::Block => {
                let (rest, block): (&[u8], WrappedBlock) =
                    WRAPPED_BLOCK_DESERIALIZER.deserialize(&buffer[cursor..])?;
                cursor += buffer[cursor..].len() - rest.len();
                Message::Block(block)
            }
            MessageTypeId::BlockHeader => {
                let (rest, header): (&[u8], WrappedHeader) =
                    WRAPPED_BLOCK_HEADER_DESERIALIZER.deserialize(&buffer[cursor..])?;
                cursor += buffer[cursor..].len() - rest.len();
                Message::BlockHeader(header)
            }
            MessageTypeId::AskForBlocks => {
                let (length, delta) =
                    u32::from_varint_bytes_bounded(&buffer[cursor..], max_ask_blocks_per_message)?;
                cursor += delta;
                // hash list
                let mut list: Vec<BlockId> = Vec::with_capacity(length as usize);
                for _ in 0..length {
                    let b_id = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?);
                    cursor += BLOCK_ID_SIZE_BYTES;
                    list.push(b_id);
                }
                Message::AskForBlocks(list)
            }
            MessageTypeId::AskPeerList => Message::AskPeerList,
            MessageTypeId::PeerList => {
                // length
                let (length, delta) =
                    u32::from_varint_bytes_bounded(&buffer[cursor..], max_peer_list_length)?;
                cursor += delta;
                // peer list
                let mut peers: Vec<IpAddr> = Vec::with_capacity(length as usize);
                for _ in 0..length {
                    let (rest, ip) = IP_DESERIALIZER
                        .deserialize::<DeserializeError>(&buffer[cursor..])
                        .map_err(|_| {
                            ModelsError::DeserializeError(
                                "Failed to deserialize IpAddr".to_string(),
                            )
                        })?;
                    cursor += buffer[cursor..].len() - rest.len();
                    peers.push(ip);
                }
                Message::PeerList(peers)
            }
            MessageTypeId::BlockNotFound => {
                let b_id = BlockId::from_bytes(&array_from_slice(&buffer[cursor..])?);
                cursor += BLOCK_ID_SIZE_BYTES;
                Message::BlockNotFound(b_id)
            }
            MessageTypeId::Operations => {
                let (rest, operations) = OPERATIONS_DESERIALIZER.deserialize(&buffer[cursor..])?;
                cursor += buffer[cursor..].len() - rest.len();
                Message::Operations(operations)
            }
            MessageTypeId::AskForOperations => {
                let (rest, operation_prefix_ids) =
                    OPERATION_PREFIX_ID_DESERIALIZER.deserialize(&buffer[cursor..])?;
                cursor += buffer[cursor..].len() - rest.len();
                Message::AskForOperations(operation_prefix_ids)
            }
            MessageTypeId::OperationsAnnouncement => {
                let (rest, operation_prefix_ids) =
                    OPERATION_PREFIX_ID_DESERIALIZER.deserialize(&buffer[cursor..])?;
                cursor += buffer[cursor..].len() - rest.len();
                Message::OperationsAnnouncement(operation_prefix_ids)
            }
            MessageTypeId::Endorsements => {
                // length
                let (length, delta) = u32::from_varint_bytes_bounded(
                    &buffer[cursor..],
                    max_endorsements_per_message,
                )?;
                cursor += delta;
                // operations
                let mut endorsements = Vec::with_capacity(length as usize);
                let endorsement_deserializer = maybe_static::maybe_static!(
                    Some(max_endorsements_per_message),
                    WrappedDeserializer<Endorsement, EndorsementDeserializer>,
                    |max: u32| WrappedDeserializer::new(EndorsementDeserializer::new(max))
                )?;
                for _ in 0..length {
                    let (rest, endorsement) =
                        endorsement_deserializer.deserialize(&buffer[cursor..])?;
                    cursor += buffer[cursor..].len() - rest.len();
                    endorsements.push(endorsement);
                }
                Message::Endorsements(endorsements)
            }
        };
        Ok((res, cursor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let mut random_bytes = [0u8; 32];
        StdRng::from_entropy().fill_bytes(&mut random_bytes);
        let keypair = KeyPair::generate();
        let msg = Message::HandshakeInitiation {
            public_key: keypair.get_public_key(),
            random_bytes,
            version: Version::from_str("TEST.1.2").unwrap(),
        };
        let ser = msg.to_bytes_compact().unwrap();
        let (deser, _) = Message::from_bytes_compact(&ser).unwrap();
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
