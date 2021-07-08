use super::error::ModelsError;
use super::operation::Operation;
use crypto::signature::Signature;
use crypto::{hash::Hash, signature::PublicKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
    pub signature: Signature,
}

impl Block {
    pub fn into_bytes(&self) -> Vec<u8> {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        self.serialize(&mut s).unwrap();
        s.take_buffer()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub creator: PublicKey,
    pub thread_number: u8,
    pub period_number: u64,
    pub roll_number: u32,
    pub parents: Vec<Hash>,
    pub endorsements: Vec<Option<Signature>>,
    pub out_ledger_hash: Hash,
    pub operation_merkle_root: Hash, // all operations hash
}

impl BlockHeader {
    pub fn compute_hash(&self) -> Result<Hash, ModelsError> {
        let mut serializer = flexbuffers::FlexbufferSerializer::new();
        self.serialize(&mut serializer)
            .map_err(|_| ModelsError::HeaderhashError)?;
        Ok(Hash::hash(&serializer.take_buffer()))
    }
}
