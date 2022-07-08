// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Contains definitions of commands used by the controller
use massa_graph::ledger::ConsensusLedgerSubset;
use massa_graph::{BlockGraphExport, BootstrapableGraph, ExportBlockStatus, Status};
use massa_models::{address::AddressState, api::EndorsementInfo, EndorsementId, OperationId};
use massa_models::{clique::Clique, stats::ConsensusStats};
use massa_models::{
    Address, BlockId, OperationSearchResult, Slot, StakersCycleProductionStats, WrappedEndorsement,
};

use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_signature::KeyPair;

use massa_models::prehash::{Map, Set};
use tokio::sync::oneshot;

use crate::{error::ConsensusResult as Result, ConsensusError, SelectionDraws};

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
    /// Returns through a channel the list of slots with the address of the selected staker.
    GetSelectionDraws {
        /// start slot
        start: Slot,
        /// end slot
        end: Slot,
        /// response channel
        response_tx: oneshot::Sender<Result<SelectionDraws, ConsensusError>>,
    },
    /// Returns the bootstrap state
    GetBootstrapState(oneshot::Sender<(ExportProofOfStake, BootstrapableGraph)>),
    /// Returns a part of the ledger
    GetLedgerPart {
        /// Start address
        start_address: Option<Address>,
        /// Size of the part of the ledger
        batch_size: usize,
        /// response channel
        response_tx: oneshot::Sender<ConsensusLedgerSubset>,
    },
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
        response_tx: oneshot::Sender<Map<OperationId, OperationSearchResult>>,
    },
    /// Get some information on operations by operation ids
    GetOperations {
        /// wanted ids
        operation_ids: Set<OperationId>,
        /// response channel
        response_tx: oneshot::Sender<Map<OperationId, OperationSearchResult>>,
    },
    /// get current stats on consensus
    GetStats(oneshot::Sender<ConsensusStats>),
    /// Get all stakers
    GetActiveStakers(oneshot::Sender<Map<Address, u64>>),
    /// Add keys to use them for staking
    RegisterStakingKeys(Vec<KeyPair>),
    /// Remove associated staking keys
    RemoveStakingAddresses(Set<Address>),
    /// Get staking addresses
    GetStakingAddresses(oneshot::Sender<Set<Address>>),
    /// Get production stats for addresses
    GetStakersProductionStats {
        /// wanted addresses
        addrs: Set<Address>,
        /// response channel
        response_tx: oneshot::Sender<Vec<StakersCycleProductionStats>>,
    },
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
        response_tx: oneshot::Sender<Map<EndorsementId, WrappedEndorsement>>,
    },
    /// get endorsements by id
    GetEndorsementsById {
        /// Wanted endorsement ids
        endorsements: Set<EndorsementId>,
        /// response channel
        response_tx: oneshot::Sender<Map<EndorsementId, EndorsementInfo>>,
    },
    /// Get cliques
    GetCliques(oneshot::Sender<Vec<Clique>>),
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusManagementCommand {}
