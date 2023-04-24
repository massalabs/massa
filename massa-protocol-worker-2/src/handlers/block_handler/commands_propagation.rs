use massa_models::block_id::BlockId;
use massa_storage::Storage;

/// Commands that the block handler can process
#[derive(Debug)]
pub enum BlockHandlerPropagationCommand {
    Stop,
    /// Notify block integration of a given block.
    IntegratedBlock {
        /// block id
        block_id: BlockId,
        /// block storage
        storage: Storage,
    },
    /// A block, or it's header, amounted to an attempted attack.
    AttackBlockDetected(BlockId),
}
