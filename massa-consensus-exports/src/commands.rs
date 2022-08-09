// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Contains definitions of commands used by the controller
use massa_graph::{BlockGraphExport, BootstrapableGraph, ExportBlockStatus, Status};
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
    /// Returns through a channel full block and status with specified hash.
    GetBlockStatus {
        /// wanted block id
        block_id: BlockId,
        /// response channel
        response_tx: oneshot::Sender<Option<ExportBlockStatus>>,
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
    /// Get some information on operation by involved addresses
    GetRecentOperations {
        /// wanted address
        address: Address,
        /// response channel
        response_tx: oneshot::Sender<(Map<OperationId, OperationSearchResult>, Storage)>,
    },
    /// Get some information on operations by operation ids
    GetOperations {
        /// wanted ids
        operation_ids: Set<OperationId>,
        /// response channel
        response_tx: oneshot::Sender<(Map<OperationId, OperationSearchResult>, Storage)>,
    },
    /// get current stats on consensus
    GetStats(oneshot::Sender<ConsensusStats>),
    /// Get block id and status by block creator address
    GetBlockIdsByCreator {
        /// wanted address
        address: Address,
        /// response channel
        response_tx: oneshot::Sender<Map<BlockId, Status>>,
    },
    /// Get Endorsements by involved addresses
    GetEndorsementsByAddress {
        /// wanted address
        address: Address,
        /// response channel
        response_tx: oneshot::Sender<(Map<EndorsementId, WrappedEndorsement>, Storage)>,
    },
    /// get endorsements by id
    GetEndorsementsById {
        /// Wanted endorsement ids
        endorsements: Set<EndorsementId>,
        /// response channel
        response_tx: oneshot::Sender<(Map<EndorsementId, EndorsementInfo>, Storage)>,
    },
    /// Get cliques
    GetCliques(oneshot::Sender<Vec<Clique>>),
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusManagementCommand {}
