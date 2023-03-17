use crossbeam::channel::Sender;
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
};
use massa_protocol_exports_2::{ProtocolController, ProtocolError};
use massa_storage::Storage;

use crate::handlers::{
    block_handler::commands::BlockHandlerCommand,
    endorsement_handler::commands::EndorsementHandlerCommand,
    operation_handler::commands::OperationHandlerCommand,
};

#[derive(Clone)]
pub struct ProtocolControllerImpl {
    pub sender_block_handler: Sender<BlockHandlerCommand>,
    pub sender_operation_handler: Sender<OperationHandlerCommand>,
    pub sender_endorsement_handler: Sender<EndorsementHandlerCommand>,
}

impl ProtocolControllerImpl {
    pub fn new(
        sender_block_handler: Sender<BlockHandlerCommand>,
        sender_operation_handler: Sender<OperationHandlerCommand>,
        sender_endorsement_handler: Sender<EndorsementHandlerCommand>,
    ) -> Self {
        ProtocolControllerImpl {
            sender_block_handler,
            sender_operation_handler,
            sender_endorsement_handler,
        }
    }
}

impl ProtocolController for ProtocolControllerImpl {
    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * `block_id`: ID of the block
    /// * `storage`: Storage instance containing references to the block and all its dependencies
    fn integrated_block(&self, block_id: BlockId, storage: Storage) -> Result<(), ProtocolError> {
        self.sender_block_handler
            .send(BlockHandlerCommand::IntegratedBlock { block_id, storage })
            .map_err(|_| ProtocolError::ChannelError("integrated_block command send error".into()))
    }

    /// Notify to protocol an attack attempt.
    fn notify_block_attack(&self, block_id: BlockId) -> Result<(), ProtocolError> {
        self.sender_block_handler
            .send(BlockHandlerCommand::AttackBlockDetected(block_id))
            .map_err(|_| {
                ProtocolError::ChannelError("notify_block_attack command send error".into())
            })
    }

    /// update the block wish list
    fn send_wishlist_delta(
        &self,
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        remove: PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError> {
        self.sender_block_handler
            .send(BlockHandlerCommand::WishlistDelta { new, remove })
            .map_err(|_| {
                ProtocolError::ChannelError("send_wishlist_delta command send error".into())
            })
    }

    /// Propagate a batch of operation ids (from pool).
    ///
    /// note: Full `OperationId` is replaced by a `OperationPrefixId` later by the worker.
    fn propagate_operations(&self, operations: Storage) -> Result<(), ProtocolError> {
        self.sender_operation_handler
            .send(OperationHandlerCommand::PropagateOperations(operations))
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_operations command send error".into())
            })
    }

    /// propagate endorsements to connected node
    fn propagate_endorsements(&self, endorsements: Storage) -> Result<(), ProtocolError> {
        self.sender_endorsement_handler
            .send(EndorsementHandlerCommand::PropagateEndorsements(
                endorsements,
            ))
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_endorsements command send error".into())
            })
    }

    fn clone_box(&self) -> Box<dyn ProtocolController> {
        Box::new(self.clone())
    }
}
