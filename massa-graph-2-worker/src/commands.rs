use massa_models::{
    block::{BlockHeader, BlockId},
    slot::Slot,
    wrapped::Wrapped,
};
use massa_storage::Storage;

#[allow(clippy::large_enum_variant)]
pub enum GraphCommand {
    RegisterBlock(BlockId, Slot, Storage),
    RegisterBlockHeader(BlockId, Wrapped<BlockHeader, BlockId>),
    MarkInvalidBlock(BlockId, Wrapped<BlockHeader, BlockId>)
}
