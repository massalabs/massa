// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Contains definitions of commands used by the controller
use massa_graph::{BlockGraphExport, BootstrapableGraph, ExportBlockStatus, Status};
use massa_models::api::BlockGraphStatus;
use massa_models::prehash::{Map, Set};
use massa_models::{address::AddressState, api::EndorsementInfo, EndorsementId, OperationId};
use massa_models::{clique::Clique, stats::ConsensusStats};
use massa_models::{Address, BlockId, OperationSearchResult, Slot, WrappedEndorsement};
use massa_storage::Storage;
use tokio::sync::oneshot;

/// Commands that can be processed by consensus.
#[derive(Debug)]
pub enum ConsensusCommand {
    /// Returns through a channel current blockgraph without block operations.
    GetBlockGraphStatus {
        /// optional start slot
        slot_start: Option<Slot>,
        /// optional end slot
        slot_end: Option<Slot>,
        /// response channel
        response_tx: oneshot::Sender<BlockGraphExport>,
    },
    /// Returns through a channel the graph statuses of a batch of blocks
    GetBlockStatuses {
        /// wanted block IDs
        ids: Vec<BlockId>,
        /// response channel
        response_tx: oneshot::Sender<Vec<BlockGraphStatus>>,
    },
    /// Returns the bootstrap state
    GetBootstrapState(oneshot::Sender<BootstrapableGraph>),
    /// Returns info for a set of addresses (rolls and balance)
    GetAddressesInfo {
        /// wanted addresses
        addresses: Set<Address>,
        /// response channel
        response_tx: oneshot::Sender<Map<Address, AddressState>>,
    },
    /// get current stats on consensus
    GetStats(oneshot::Sender<ConsensusStats>),
    /// Get a block at a given slot in a blockclique
    GetBlockcliqueBlockAtSlot {
        /// wanted slot
        slot: Slot,
        /// response channel
        response_tx: oneshot::Sender<Option<BlockId>>,
    },
    /// Get the best parents and their period
    GetBestParents {
        /// response channel
        response_tx: oneshot::Sender<Vec<(BlockId, u64)>>,
    },
    /// Send a block
    SendBlock {
        /// block id
        block_id: BlockId,
        /// block slot
        slot: Slot,
        /// All the objects for the block
        block_storage: Storage,
        /// response channel
        response_tx: oneshot::Sender<()>,
    },
    /// Get cliques
    GetCliques(oneshot::Sender<Vec<Clique>>),
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusManagementCommand {}
