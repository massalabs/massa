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

impl Clone for BlockHandlerPropagationCommand {
    fn clone(&self) -> Self {
        match self {
            BlockHandlerPropagationCommand::Stop => BlockHandlerPropagationCommand::Stop,
            BlockHandlerPropagationCommand::IntegratedBlock { block_id, storage } => {
                BlockHandlerPropagationCommand::IntegratedBlock {
                    block_id: block_id.clone(),
                    storage: storage.clone("protocol".into()),
                }
            }
            BlockHandlerPropagationCommand::AttackBlockDetected(block_id) => {
                BlockHandlerPropagationCommand::AttackBlockDetected(block_id.clone())
            }
        }
    }
}
