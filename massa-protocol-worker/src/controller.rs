use std::{collections::HashMap, net::SocketAddr, time::Duration};

use crossbeam::channel::Sender;
use massa_models::{
    block_header::SecuredHeader,
    block_id::BlockId,
    prehash::{PreHashMap, PreHashSet},
    stats::NetworkStats,
};
use massa_protocol_exports::{BootstrapPeers, ProtocolController, ProtocolError};
use massa_storage::Storage;
use peernet::{peer::PeerConnectionType, peer_id::PeerId};

use crate::{
    connectivity::ConnectivityCommand,
    handlers::{
        block_handler::{
            commands_propagation::BlockHandlerPropagationCommand,
            commands_retrieval::BlockHandlerRetrievalCommand,
        },
        endorsement_handler::commands_propagation::EndorsementHandlerPropagationCommand,
        operation_handler::commands_propagation::OperationHandlerPropagationCommand,
        peer_handler::models::PeerManagementCmd,
    },
};

#[derive(Clone)]
pub(crate) struct ProtocolControllerImpl {
    // Use Option here in order to be able to drop the Sender without dropping the controller
    // using `option.take()`.
    // This is needed as to be able to stop the controller, the Sender has to be dropped,
    // if not, the handler will deadlock on `recv`
    // As this is never None, we allow ourselves to use `unwrap` to acceed to the senders
    pub(crate) sender_block_retrieval_handler: Option<Sender<BlockHandlerRetrievalCommand>>,
    pub(crate) sender_block_handler: Option<Sender<BlockHandlerPropagationCommand>>,
    pub(crate) sender_operation_handler: Option<Sender<OperationHandlerPropagationCommand>>,
    pub(crate) sender_endorsement_handler: Option<Sender<EndorsementHandlerPropagationCommand>>,
    pub(crate) sender_connectivity_thread: Option<Sender<ConnectivityCommand>>,
    pub(crate) sender_peer_management_thread: Option<Sender<PeerManagementCmd>>,
}

impl ProtocolControllerImpl {
    pub(crate) fn new(
        sender_block_retrieval_handler: Sender<BlockHandlerRetrievalCommand>,
        sender_block_handler: Sender<BlockHandlerPropagationCommand>,
        sender_operation_handler: Sender<OperationHandlerPropagationCommand>,
        sender_endorsement_handler: Sender<EndorsementHandlerPropagationCommand>,
        sender_connectivity_thread: Sender<ConnectivityCommand>,
        sender_peer_management_thread: Sender<PeerManagementCmd>,
    ) -> Self {
        ProtocolControllerImpl {
            sender_block_retrieval_handler: Some(sender_block_retrieval_handler),
            sender_block_handler: Some(sender_block_handler),
            sender_operation_handler: Some(sender_operation_handler),
            sender_endorsement_handler: Some(sender_endorsement_handler),
            sender_connectivity_thread: Some(sender_connectivity_thread),
            sender_peer_management_thread: Some(sender_peer_management_thread),
        }
    }
}

impl ProtocolController for ProtocolControllerImpl {
    fn stop(&mut self) {
        drop(self.sender_block_handler.take());
        drop(self.sender_operation_handler.take());
        drop(self.sender_endorsement_handler.take());
        drop(self.sender_block_retrieval_handler.take());
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
            .send(BlockHandlerPropagationCommand::IntegratedBlock { block_id, storage })
            .map_err(|_| ProtocolError::ChannelError("integrated_block command send error".into()))
    }

    /// Notify to protocol an attack attempt.
    fn notify_block_attack(&self, block_id: BlockId) -> Result<(), ProtocolError> {
        self.sender_block_handler
            .as_ref()
            .unwrap()
            .send(BlockHandlerPropagationCommand::AttackBlockDetected(
                block_id,
            ))
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

    fn get_stats(
        &self,
    ) -> Result<
        (
            NetworkStats,
            HashMap<PeerId, (SocketAddr, PeerConnectionType)>,
        ),
        ProtocolError,
    > {
        let (sender, receiver) = crossbeam::channel::bounded(1);
        self.sender_connectivity_thread
            .as_ref()
            .unwrap()
            .send(ConnectivityCommand::GetStats { responder: sender })
            .map_err(|_| ProtocolError::ChannelError("get_stats command send error".into()))?;
        receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| ProtocolError::ChannelError("get_stats command receive error".into()))
    }

    fn ban_peers(&self, peer_ids: Vec<PeerId>) -> Result<(), ProtocolError> {
        self.sender_peer_management_thread
            .as_ref()
            .unwrap()
            .send(PeerManagementCmd::Ban(peer_ids))
            .map_err(|_| ProtocolError::ChannelError("ban_peers command send error".into()))
    }

    fn unban_peers(&self, peer_ids: Vec<PeerId>) -> Result<(), ProtocolError> {
        self.sender_peer_management_thread
            .as_ref()
            .unwrap()
            .send(PeerManagementCmd::Unban(peer_ids))
            .map_err(|_| ProtocolError::ChannelError("unban_peers command send error".into()))
    }

    fn get_bootstrap_peers(&self) -> Result<BootstrapPeers, ProtocolError> {
        let (sender, receiver) = crossbeam::channel::bounded(1);
        self.sender_peer_management_thread
            .as_ref()
            .unwrap()
            .send(PeerManagementCmd::GetBootstrapPeers { responder: sender })
            .map_err(|_| {
                ProtocolError::ChannelError("get_bootstrap_peers command send error".into())
            })?;
        receiver.recv_timeout(Duration::from_secs(10)).map_err(|_| {
            ProtocolError::ChannelError("get_bootstrap_peers command receive error".into())
        })
    }

    fn clone_box(&self) -> Box<dyn ProtocolController> {
        Box::new(self.clone())
    }
}
