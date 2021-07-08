use crate::crypto::hash::Hash;
use crate::crypto::signature::Signature;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endorsement(EndorsementContent, Signature);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndorsementContent {
    pub thread_number: u32,
    pub slot_number: u64,
    pub block_hash: Hash,
    pub endorser_roll_number: u32,
}
