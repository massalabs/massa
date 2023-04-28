// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::HashMap;
use std::net::SocketAddr;

use crate::error::ProtocolError;
use crate::BootstrapPeers;

use massa_models::prehash::{PreHashMap, PreHashSet};
use massa_models::stats::NetworkStats;
use massa_models::{block_header::SecuredHeader, block_id::BlockId};
use massa_storage::Storage;
use peernet::peer::PeerConnectionType;
use peernet::peer_id::PeerId;

#[cfg_attr(any(test, feature = "testing"), mockall::automock)]
pub trait ProtocolController: Send + Sync {
    /// Perform all operations needed to stop the ProtocolController
    /// without dropping it completely yet.
    fn stop(&mut self);

    /// Sends the order to propagate the header of a block
    ///
    /// # Arguments
    /// * `block_id`: ID of the block
    /// * `storage`: Storage instance containing references to the block and all its dependencies
    fn integrated_block(&self, block_id: BlockId, storage: Storage) -> Result<(), ProtocolError>;

    /// Notify to protocol an attack attempt.
    ///
    /// # Arguments
    /// * `block_id`: ID of the block
    fn notify_block_attack(&self, block_id: BlockId) -> Result<(), ProtocolError>;

    /// Update the block wish list
    ///
    /// # Arguments
    /// * `new`: new blocks to add to the wish list
    /// * `remove`: blocks to remove from the wish list
    fn send_wishlist_delta(
        &self,
        new: PreHashMap<BlockId, Option<SecuredHeader>>,
        remove: PreHashSet<BlockId>,
    ) -> Result<(), ProtocolError>;

    /// Propagate a batch of operation (from pool).
    /// note: Full `OperationId` is replaced by a `OperationPrefixId` later by the worker.
    ///
    /// # Arguments:
    /// * `operations`: operations to propagate
    fn propagate_operations(&self, operations: Storage) -> Result<(), ProtocolError>;

    /// Propagate a batch of endorsement (from pool).
    ///
    /// # Arguments:
    /// * `endorsements`: endorsements to propagate
    fn propagate_endorsements(&self, endorsements: Storage) -> Result<(), ProtocolError>;

    /// Get the stats from the protocol
    /// Returns a tuple containing the stats and the list of peers
    fn get_stats(
        &self,
    ) -> Result<
        (
            NetworkStats,
            HashMap<PeerId, (SocketAddr, PeerConnectionType)>,
        ),
        ProtocolError,
    >;

    /// Get a list of peers to be sent to someone that bootstrap to us
    fn get_bootstrap_peers(&self) -> Result<BootstrapPeers, ProtocolError>;

    /// Ban a list of Peer Id
    fn ban_peers(&self, peer_ids: Vec<PeerId>) -> Result<(), ProtocolError>;

    /// Unban a list of Peer Id
    fn unban_peers(&self, peer_ids: Vec<PeerId>) -> Result<(), ProtocolError>;

    /// Returns a boxed clone of self.
    /// Useful to allow cloning `Box<dyn ProtocolController>`.
    fn clone_box(&self) -> Box<dyn ProtocolController>;
}

/// Allow cloning `Box<dyn ProtocolController>`
/// Uses `ProtocolController::clone_box` internally
impl Clone for Box<dyn ProtocolController> {
    fn clone(&self) -> Box<dyn ProtocolController> {
        self.clone_box()
    }
}

/// Protocol manager used to stop the protocol
pub trait ProtocolManager {
    /// Stop the protocol
    /// Note that we do not take self by value to consume it
    /// because it is not allowed to move out of Box<dyn ProtocolManager>
    /// This will improve if the `unsized_fn_params` feature stabilizes enough to be safely usable.
    fn stop(&mut self);
}
