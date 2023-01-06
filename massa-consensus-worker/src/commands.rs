use massa_models::{
    block::{BlockHeader, BlockId},
    secure_share::SecureShare,
    slot::Slot,
};
use massa_storage::Storage;

#[allow(clippy::large_enum_variant)]
pub enum ConsensusCommand {
    RegisterBlock(BlockId, Slot, Storage, bool),
    RegisterBlockHeader(BlockId, SecureShare<BlockHeader, BlockId>),
    MarkInvalidBlock(BlockId, SecureShare<BlockHeader, BlockId>),
}
