use crypto::signature::{PublicKey, Signature};
use models::block::Block;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

pub const HANDSHAKE_RANDOMNES_SIZE_BYTES: usize = 32;

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    HandshakeInitiation {
        public_key: PublicKey,
        random_bytes: [u8; HANDSHAKE_RANDOMNES_SIZE_BYTES],
    },
    HandshakeReply {
        signature: Signature,
    },
    Block(Block),
    Transaction(String),
    AskPeerList,
    PeerList(Vec<IpAddr>),
}
