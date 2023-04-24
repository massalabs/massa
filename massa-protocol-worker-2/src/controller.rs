use crossbeam::channel::Sender;
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
};
use massa_protocol_exports_2::{ProtocolController, ProtocolError};
use massa_storage::Storage;

use crate::handlers::{
    block_handler::{
        commands_propagation::BlockHandlerCommand, commands_retrieval::BlockHandlerRetrievalCommand,
    },
    endorsement_handler::commands_propagation::EndorsementHandlerPropagationCommand,
    operation_handler::commands_propagation::OperationHandlerPropagationCommand,
};

#[derive(Clone)]
pub struct ProtocolControllerImpl {
    // Use Option here in order to be able to drop the Sender without dropping the controller
    // using `option.take()`.
    // This is needed as to be able to stop the controller, the Sender has to be dropped,
    // if not, the handler will deadlock on `recv`
    // As this is never None, we allow ourselves to use `unwrap` to acceed to the senders
    pub sender_block_retrieval_handler: Option<Sender<BlockHandlerRetrievalCommand>>,
    pub sender_block_handler: Option<Sender<BlockHandlerCommand>>,
    pub sender_operation_handler: Option<Sender<OperationHandlerPropagationCommand>>,
    pub sender_endorsement_handler: Option<Sender<EndorsementHandlerPropagationCommand>>,
}

impl ProtocolControllerImpl {
    pub fn new(
        sender_block_retrieval_handler: Sender<BlockHandlerRetrievalCommand>,
        sender_block_handler: Sender<BlockHandlerCommand>,
        sender_operation_handler: Sender<OperationHandlerPropagationCommand>,
        sender_endorsement_handler: Sender<EndorsementHandlerPropagationCommand>,
    ) -> Self {
        ProtocolControllerImpl {
            sender_block_retrieval_handler: Some(sender_block_retrieval_handler),
            sender_block_handler: Some(sender_block_handler),
            sender_operation_handler: Some(sender_operation_handler),
            sender_endorsement_handler: Some(sender_endorsement_handler),
        }
    }
}

impl ProtocolController for ProtocolControllerImpl {
    fn stop(&mut self) {
        drop(self.sender_block_handler.take());
        drop(self.sender_operation_handler.take());
        drop(self.sender_endorsement_handler.take());
    }

    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * `block_id`: ID of the block
    /// * `storage`: Storage instance containing references to the block and all its dependencies
    fn integrated_block(&self, block_id: BlockId, storage: Storage) -> Result<(), ProtocolError> {
        self.sender_block_handler
            .as_ref()
            .unwrap()
            .send(BlockHandlerCommand::IntegratedBlock { block_id, storage })
            .map_err(|_| ProtocolError::ChannelError("integrated_block command send error".into()))
    }

    /// Notify to protocol an attack attempt.
    fn notify_block_attack(&self, block_id: BlockId) -> Result<(), ProtocolError> {
        self.sender_block_handler
            .as_ref()
            .unwrap()
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
        self.sender_block_retrieval_handler
            .as_ref()
            .unwrap()
            .send(BlockHandlerRetrievalCommand::WishlistDelta { new, remove })
            .map_err(|_| {
                ProtocolError::ChannelError("send_wishlist_delta command send error".into())
            })
    }

    /// Propagate a batch of operation ids (from pool).
    ///
    /// note: Full `OperationId` is replaced by a `OperationPrefixId` later by the worker.
    fn propagate_operations(&self, operations: Storage) -> Result<(), ProtocolError> {
        //TODO: Change when send will be in propagation
        let operations = operations.get_op_refs().clone();
        self.sender_operation_handler
            .as_ref()
            .unwrap()
            .send(OperationHandlerPropagationCommand::AnnounceOperations(
                operations,
            ))
            .map_err(|_| {
                ProtocolError::ChannelError("propagate_operations command send error".into())
            })
    }

    /// propagate endorsements to connected node
    fn propagate_endorsements(&self, endorsements: Storage) -> Result<(), ProtocolError> {
        self.sender_endorsement_handler
            .as_ref()
            .unwrap()
            .send(EndorsementHandlerPropagationCommand::PropagateEndorsements(
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
