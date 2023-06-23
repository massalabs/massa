use massa_models::{
    block_header::BlockHeader, block_id::BlockId, secure_share::SecureShare, slot::Slot,
};
use massa_storage::Storage;

#[allow(clippy::large_enum_variant)]
pub enum ConsensusCommand {
    RegisterBlock(BlockId, Slot, Storage, bool),
    RegisterBlockHeader(BlockId, SecureShare<BlockHeader, BlockId>),
    MarkInvalidBlock(BlockId, SecureShare<BlockHeader, BlockId>),
}

impl Clone for ConsensusCommand {
    fn clone(&self) -> Self {
        match self {
            ConsensusCommand::RegisterBlock(block_id, slot, storage, is_new) => {
                ConsensusCommand::RegisterBlock(
                    block_id.clone(),
                    *slot,
                    storage.clone("consensus".into()),
                    *is_new,
                )
            }
            ConsensusCommand::RegisterBlockHeader(block_id, share) => {
                ConsensusCommand::RegisterBlockHeader(block_id.clone(), share.clone())
            }
            ConsensusCommand::MarkInvalidBlock(block_id, share) => {
                ConsensusCommand::MarkInvalidBlock(block_id.clone(), share.clone())
            }
        }
    }
}
