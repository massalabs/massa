use massa_models::{
    block::{BlockHeader, BlockId},
    slot::Slot,
    wrapped::Wrapped,
};
use massa_storage::Storage;

pub enum GraphCommand {
    GetBlockGraphStatus(Option<Slot>, Option<Slot>),
    GetBlockStatuses(Vec<BlockId>),
    GetCliques,
    GetBootstrapGraph,
    GetStats,
    GetBestParents,
    GetBlockCliqueBlockAtSlot(Slot),
    GetLatestBlockCliqueBlockAtSlot(Slot),
    RegisterBlock(BlockId, Slot, Storage),
    RegisterBlockHeader(BlockId, Wrapped<BlockHeader, BlockId>),
    Stop,
}
