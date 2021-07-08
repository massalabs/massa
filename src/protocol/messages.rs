use crate::crypto::signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

#[derive(Debug, Serialize, Deserialize)]
pub enum Message {
    HandshakeInitiation {
        public_key: PublicKey,
        random_bytes: Vec<u8>,
    },
    HandshakeReply {
        signature: Signature,
    },
    Block(String),
    Transaction(String),
    AskPeerList,
    PeerList(Vec<IpAddr>),
}
