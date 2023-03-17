use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
};
use massa_storage::Storage;

/// Commands that the block handler can process
#[derive(Debug)]
pub enum BlockHandlerCommand {
    /// Notify block integration of a given block.
    IntegratedBlock {
        /// block id
        block_id: BlockId,
        /// block storage
        storage: Storage,
    },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(BlockId),
    /// Wish list delta
    WishlistDelta {
        /// add to wish list
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        /// remove from wish list
        remove: PreHashSet<BlockId>,
    },
}
