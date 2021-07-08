use super::{error::ModelsError, operation::Operation, slot::Slot};
use crypto::{
    hash::Hash,
    signature::{PublicKey, Signature},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub operations: Vec<Operation>,
    pub signature: Signature,
}

impl Block {
    pub fn into_bytes(&self) -> Result<Vec<u8>, ModelsError> {
        let mut s = flexbuffers::FlexbufferSerializer::new();
        self.serialize(&mut s)?;
        Ok(s.take_buffer())
    }

    pub fn from_bytes(data: &[u8]) -> Result<Block, ModelsError> {
        let r = flexbuffers::Reader::get_root(&data)?;
        Block::deserialize(r).map_err(|e| ModelsError::from(e))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub creator: PublicKey,
    pub slot: Slot,
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
