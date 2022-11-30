use massa_models::{
    block::{BlockHeader, BlockId},
    slot::Slot,
    wrapped::Wrapped,
};
use massa_storage::Storage;

#[allow(clippy::large_enum_variant)]
pub enum ConsensusCommand {
    RegisterBlock(BlockId, Slot, Storage, bool),
    RegisterBlockHeader(BlockId, Wrapped<BlockHeader, BlockId>),
    MarkInvalidBlock(BlockId, Wrapped<BlockHeader, BlockId>),
}
