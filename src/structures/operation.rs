use crate::crypto::hash::Hash;
use crate::crypto::signature::{PublicKey, Signature};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Operation {
    Transaction(TransactionContent, Signature), // Sender Signature
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionContent {
    expire_slot: u64,
    sender_pubkey: PublicKey,
    receiver_addr: Hash, // Hash of its publickey
    amount: u64,
    fee: u64,
}
