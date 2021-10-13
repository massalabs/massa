// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(bool_to_option)]
#![feature(hash_drain_filter)]
#![feature(map_first_last)]

#[macro_use]
extern crate logging;
pub use block_graph::BootstrapableGraph;
pub use block_graph::{
    BlockGraphExport, DiscardReason, ExportActiveBlock, ExportBlockStatus, ExportCompiledBlock,
    LedgerDataExport, Status,
};
pub use config::ConsensusConfig;
pub use consensus_controller::{
    start_consensus_controller, ConsensusCommandSender, ConsensusEventReceiver, ConsensusManager,
};
pub use consensus_worker::{ConsensusCommand, ConsensusEvent, ConsensusStats};
pub use error::ConsensusError;
pub use ledger::LedgerSubset;
pub use models::address::AddressState;
pub use models::clique::Clique;
pub use models::ledger::LedgerChange;
pub use pos::{ExportProofOfStake, RollCounts, RollUpdate, RollUpdates, ThreadCycleState};

mod block_graph;
mod config;
mod consensus_controller;
mod consensus_worker;
pub mod error;
pub mod ledger;
mod pos;
#[cfg(test)]
mod tests;
