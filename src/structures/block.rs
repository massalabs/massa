use super::operation::Operation;
use crate::crypto::hash::Hash;
use crate::crypto::signature::Signature;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block(pub BlockContent, pub Signature); // block creator signature

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockContent {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub thread_number: u8,
    pub slot_number: u64,
    pub roll_number: u32,
    pub parents: Vec<Hash>,
    pub endorsements: Vec<Option<Signature>>,
    pub out_ledger_hash: Hash,
}
