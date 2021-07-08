use crypto::signature::{PublicKey, Signature};
use models::block::Block;
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
    /// Isolated transaction that should be included in a block eventually.
    Transaction(String),
    /// Message asking the peer for its advertisable peers list.
    AskPeerList,
    /// Reply to a AskPeerList message
    /// Peers are ordered from most to less reliable.
    /// If the ip of the node that sent that message is routable,
    /// it is the first ip of the list.
    PeerList(Vec<IpAddr>),
}
