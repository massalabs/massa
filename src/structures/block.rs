use super::operation::Operation;
use crate::crypto::signature::Signature;
use crate::crypto::{hash::Hash, signature::PublicKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
    pub signature: Signature,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub creator: PublicKey,
    pub thread_number: u8,
    pub slot_number: u64,
    pub roll_number: u32,
    pub parents: Vec<Hash>,
    pub endorsements: Vec<Option<Signature>>,
    pub out_ledger_hash: Hash,
    pub operation_merkle_root: Hash, // all operations hash
}
