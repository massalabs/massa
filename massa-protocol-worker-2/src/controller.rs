use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    endorsement::EndorsementId,
    operation::OperationId,
    prehash::{PreHashMap, PreHashSet},
};
use massa_protocol_exports_2::{ProtocolController, ProtocolError};
use massa_storage::Storage;

#[derive(Clone)]
pub struct ProtocolControllerImpl {}

impl ProtocolControllerImpl {
    pub fn new() -> Self {
        ProtocolControllerImpl {}
    }
}

/// block result: map block id to
/// ```md
/// Option(
///     Option(set(operation id)),
///     Option(Vec(endorsement id))
/// )
/// ```
pub type BlocksResults =
    PreHashMap<BlockId, Option<(Option<PreHashSet<OperationId>>, Option<Vec<EndorsementId>>)>>;

/// Commands that protocol worker can process
#[derive(Debug)]
pub enum ProtocolCommand {
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
    /// Propagate operations (send batches)
    /// note: `Set<OperationId>` are replaced with `OperationPrefixIds`
    ///       by the controller
    PropagateOperations(Storage),
    /// Propagate endorsements
    PropagateEndorsements(Storage),
}

impl ProtocolController for ProtocolControllerImpl {
    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * `block_id`: ID of the block
    /// * `storage`: Storage instance containing references to the block and all its dependencies
    fn integrated_block(&self, block_id: BlockId, storage: Storage) -> Result<(), ProtocolError> {
        // massa_trace!("protocol.command_sender.integrated_block", {
        //     "block_id": block_id
        // });
        // self.0
        //     .blocking_send(ProtocolCommand::IntegratedBlock { block_id, storage })
        //     .map_err(|_| ProtocolError::ChannelError("block_integrated command send error".into()))
        Ok(())
    }

    /// Notify to protocol an attack attempt.
    fn notify_block_attack(&self, block_id: BlockId) -> Result<(), ProtocolError> {
        // massa_trace!("protocol.command_sender.notify_block_attack", {
        //     "block_id": block_id
        // });
        // self.0
        //     .blocking_send(ProtocolCommand::AttackBlockDetected(block_id))
        //     .map_err(|_| {
        //         ProtocolError::ChannelError("notify_block_attack command send error".into())
        //     })
        Ok(())
    }

    /// update the block wish list
    fn send_wishlist_delta(
        &self,
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        remove: PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError> {
        // massa_trace!("protocol.command_sender.send_wishlist_delta", { "new": new, "remove": remove });
        // self.0
        //     .blocking_send(ProtocolCommand::WishlistDelta { new, remove })
        //     .map_err(|_| {
        //         ProtocolError::ChannelError("send_wishlist_delta command send error".into())
        //     })
        Ok(())
    }

    /// Propagate a batch of operation ids (from pool).
    ///
    /// note: Full `OperationId` is replaced by a `OperationPrefixId` later by the worker.
    fn propagate_operations(&self, operations: Storage) -> Result<(), ProtocolError> {
        // massa_trace!("protocol.command_sender.propagate_operations", {
        //     "operations": operations.get_op_refs()
        // });
        // self.0
        //     .blocking_send(ProtocolCommand::PropagateOperations(operations))
        //     .map_err(|_| {
        //         ProtocolError::ChannelError("propagate_operation command send error".into())
        //     })
        Ok(())
    }

    /// propagate endorsements to connected node
    fn propagate_endorsements(&self, endorsements: Storage) -> Result<(), ProtocolError> {
        // massa_trace!("protocol.command_sender.propagate_endorsements", {
        //     "endorsements": endorsements.get_endorsement_refs()
        // });
        // self.0
        //     .blocking_send(ProtocolCommand::PropagateEndorsements(endorsements))
        //     .map_err(|_| {
        //         ProtocolError::ChannelError("propagate_endorsements command send error".into())
        //     })
        Ok(())
    }

    fn clone_box(&self) -> Box<dyn ProtocolController> {
        Box::new(self.clone())
    }
}
