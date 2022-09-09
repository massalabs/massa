// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! Contains definitions of commands used by the controller
use massa_graph::{BlockGraphExport, BootstrapableGraph};
use massa_models::api::BlockGraphStatus;
use massa_models::{block::BlockId, slot::Slot};
use massa_models::{clique::Clique, stats::ConsensusStats};
use massa_storage::Storage;
use tokio::sync::{oneshot, mpsc};

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
    GetBootstrapState(mpsc::Sender<Box<BootstrapableGraph>>),
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
