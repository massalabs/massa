use crypto::{
    hash::{Hash, HASH_SIZE_BYTES},
    signature::{PublicKey, Signature, PUBLIC_KEY_SIZE_BYTES, SIGNATURE_SIZE_BYTES},
};
use models::{
    array_from_slice, Block, BlockHeader, DeserializeCompact, DeserializeVarInt, ModelsError,
    SerializationContext, SerializeCompact, SerializeVarInt,
};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

pub const HANDSHAKE_RANDOMNES_SIZE_BYTES: usize = 32;

/// All messages that can be sent or received.
#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    /// Initiates handshake.
    HandshakeInitiation {
        /// Our public_key, so the peer can decode our reply.
        public_key: PublicKey,
        /// Random data we expect the peer to sign with its private_key.
        /// They should send us their handshake initiation message to
        /// let us know their public key.
        random_bytes: [u8; HANDSHAKE_RANDOMNES_SIZE_BYTES],
    },
    /// Reply to a handskake initiation message.
    HandshakeReply {
        /// Signature of the received random bytes with our private_key.
        signature: Signature,
    },
    /// Whole block structure.
    Block(Block),
    /// Block header
    BlockHeader(BlockHeader),
    /// Message asking the peer for a block.
    AskForBlock(Hash),
    /// Message asking the peer for its advertisable peers list.
    AskPeerList,
    /// Reply to a AskPeerList message
    /// Peers are ordered from most to less reliable.
    /// If the ip of the node that sent that message is routable,
    /// it is the first ip of the list.
    PeerList(Vec<IpAddr>),
}

const HANDSHAKE_INITIATION_TYPE_ID: u32 = 0;
const HANDSHAKE_REPLY_TYPE_ID: u32 = 1;
const BLOCK_TYPE_ID: u32 = 2;
const BLOCK_HEADER_TYPE_ID: u32 = 3;
const ASK_FOR_BLOCK_TYPE_ID: u32 = 4;
const ASK_PEER_LIST_TYPE_ID: u32 = 5;
const PEER_LIST_TYPE_ID: u32 = 6;

impl SerializeCompact for Message {
    fn to_bytes_compact(&self, context: &SerializationContext) -> Result<Vec<u8>, ModelsError> {
        let mut res: Vec<u8> = Vec::new();
        match self {
            Message::HandshakeInitiation {
                public_key,
                random_bytes,
            } => {
                res.extend(HANDSHAKE_INITIATION_TYPE_ID.to_varint_bytes());
                res.extend(&public_key.to_bytes());
                res.extend(random_bytes);
            }
            Message::HandshakeReply { signature } => {
                res.extend(HANDSHAKE_REPLY_TYPE_ID.to_varint_bytes());
                res.extend(&signature.to_bytes());
            }
            Message::Block(block) => {
                res.extend(BLOCK_TYPE_ID.to_varint_bytes());
                res.extend(&block.to_bytes_compact(&context)?);
            }
            Message::BlockHeader(header) => {
                res.extend(BLOCK_HEADER_TYPE_ID.to_varint_bytes());
                res.extend(&header.to_bytes_compact(&context)?);
            }
            Message::AskForBlock(hash) => {
                res.extend(ASK_FOR_BLOCK_TYPE_ID.to_varint_bytes());
                res.extend(&hash.to_bytes());
            }
            Message::AskPeerList => {
                res.extend(ASK_PEER_LIST_TYPE_ID.to_varint_bytes());
            }
            Message::PeerList(ip_vec) => {
                res.extend(PEER_LIST_TYPE_ID.to_varint_bytes());
                res.extend((ip_vec.len() as u64).to_varint_bytes());
                for ip in ip_vec.into_iter() {
                    res.extend(ip.to_bytes_compact(&context)?)
                }
            }
        }
        Ok(res)
    }
}

impl DeserializeCompact for Message {
    fn from_bytes_compact(
        buffer: &[u8],
        context: &SerializationContext,
    ) -> Result<(Self, usize), ModelsError> {
        let mut cursor = 0usize;

        let (type_id, delta) = u32::from_varint_bytes(&buffer[cursor..])?;
        cursor += delta;

        let res = match type_id {
            HANDSHAKE_INITIATION_TYPE_ID => {
                // public key
                let public_key = PublicKey::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += PUBLIC_KEY_SIZE_BYTES;
                // random bytes
                let random_bytes: [u8; HANDSHAKE_RANDOMNES_SIZE_BYTES] =
                    array_from_slice(&buffer[cursor..])?;
                cursor += HANDSHAKE_RANDOMNES_SIZE_BYTES;
                // return message
                Message::HandshakeInitiation {
                    public_key,
                    random_bytes,
                }
            }
            HANDSHAKE_REPLY_TYPE_ID => {
                let signature = Signature::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += SIGNATURE_SIZE_BYTES;
                Message::HandshakeReply { signature }
            }
            BLOCK_TYPE_ID => {
                let (block, delta) = Block::from_bytes_compact(&buffer[cursor..], &context)?;
                cursor += delta;
                Message::Block(block)
            }
            BLOCK_HEADER_TYPE_ID => {
                let (header, delta) = BlockHeader::from_bytes_compact(&buffer[cursor..], &context)?;
                cursor += delta;
                Message::BlockHeader(header)
            }
            ASK_FOR_BLOCK_TYPE_ID => {
                let hash = Hash::from_bytes(&array_from_slice(&buffer[cursor..])?)?;
                cursor += HASH_SIZE_BYTES;
                Message::AskForBlock(hash)
            }
            ASK_PEER_LIST_TYPE_ID => Message::AskPeerList,
            PEER_LIST_TYPE_ID => {
                // length
                let (length, delta) = u32::from_varint_bytes_bounded(
                    &buffer[cursor..],
                    context.max_peer_list_length,
                )?;
                cursor += delta;
                // peer list
                let mut peers: Vec<IpAddr> = Vec::with_capacity(length as usize);
                for _ in 0..length {
                    let (ip, delta) = IpAddr::from_bytes_compact(&buffer[cursor..], &context)?;
                    cursor += delta;
                    peers.push(ip);
                }
                Message::PeerList(peers)
            }
            _ => {
                return Err(ModelsError::DeserializeError(
                    "unsupported message type".into(),
                ))
            }
        };
        Ok((res, cursor))
    }
}
