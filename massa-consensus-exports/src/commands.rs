// Copyright (c) 2021 MASSA LABS <info@massa.net>

///! Contains definitions of commands used by the controller
use massa_graph::{BlockGraphExport, BootstrapableGraph, ExportBlockStatus, Status};
use massa_models::{
    address::AddressState, api::EndorsementInfo, Endorsement, EndorsementId, OperationId,
};
use massa_models::{clique::Clique, stats::ConsensusStats};
use massa_models::{
    Address, Block, BlockId, OperationSearchResult, Slot, StakersCycleProductionStats,
};

use massa_proof_of_stake_exports::ExportProofOfStake;
use massa_signature::PrivateKey;

use massa_models::prehash::{PreHashMap, PreHashSet};
use tokio::sync::oneshot;

use crate::{error::ConsensusResult as Result, ConsensusError, SelectionDraws};

/// Commands that can be proccessed by consensus.
#[derive(Debug)]
pub enum ConsensusCommand {
    /// Returns through a channel current blockgraph without block operations.
    GetBlockGraphStatus {
        slot_start: Option<Slot>,
        slot_end: Option<Slot>,
        response_tx: oneshot::Sender<BlockGraphExport>,
    },
    /// Returns through a channel full block with specified hash.
    GetActiveBlock {
        block_id: BlockId,
        response_tx: oneshot::Sender<Option<Block>>,
    },
    /// Returns through a channel full block and status with specified hash.
    GetBlockStatus {
        block_id: BlockId,
        response_tx: oneshot::Sender<Option<ExportBlockStatus>>,
    },
    /// Returns through a channel the list of slots with the address of the selected staker.
    GetSelectionDraws {
        start: Slot,
        end: Slot,
        response_tx: oneshot::Sender<Result<SelectionDraws, ConsensusError>>,
    },
    /// Returns the bootstrap state
    GetBootstrapState(oneshot::Sender<(ExportProofOfStake, BootstrapableGraph)>),
    /// Returns info for a set of addresses (rolls and balance)
    GetAddressesInfo {
        addresses: PreHashSet<Address>,
        response_tx: oneshot::Sender<PreHashMap<Address, AddressState>>,
    },
    GetRecentOperations {
        address: Address,
        response_tx: oneshot::Sender<PreHashMap<OperationId, OperationSearchResult>>,
    },
    GetOperations {
        operation_ids: PreHashSet<OperationId>,
        response_tx: oneshot::Sender<PreHashMap<OperationId, OperationSearchResult>>,
    },
    GetStats(oneshot::Sender<ConsensusStats>),
    GetActiveStakers(oneshot::Sender<PreHashMap<Address, u64>>),
    RegisterStakingPrivateKeys(Vec<PrivateKey>),
    RemoveStakingAddresses(PreHashSet<Address>),
    GetStakingAddressses(oneshot::Sender<PreHashSet<Address>>),
    GetStakersProductionStats {
        addrs: PreHashSet<Address>,
        response_tx: oneshot::Sender<Vec<StakersCycleProductionStats>>,
    },
    GetBlockIdsByCreator {
        address: Address,
        response_tx: oneshot::Sender<PreHashMap<BlockId, Status>>,
    },
    GetEndorsementsByAddress {
        address: Address,
        response_tx: oneshot::Sender<PreHashMap<EndorsementId, Endorsement>>,
    },
    GetEndorsementsById {
        endorsements: PreHashSet<EndorsementId>,
        response_tx: oneshot::Sender<PreHashMap<EndorsementId, EndorsementInfo>>,
    },

    GetCliques(oneshot::Sender<Vec<Clique>>),
}

/// Events that are emitted by consensus.
#[derive(Debug, Clone)]
pub enum ConsensusManagementCommand {}
