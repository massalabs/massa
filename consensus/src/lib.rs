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
    ExportDiscardedBlocks, LedgerDataExport, Status,
};
pub use config::ConsensusConfig;
pub use consensus_controller::{
    start_consensus_controller, ConsensusCommandSender, ConsensusEventReceiver, ConsensusManager,
};
pub use consensus_worker::{ConsensusCommand, ConsensusEvent, ConsensusStats};
pub use error::ConsensusError;
pub use ledger::LedgerExport;
pub use models::address::AddressState;
pub use models::clique::Clique;
pub use models::clique::ExportClique;
pub use models::ledger::LedgerChange;
pub use pos::{ExportProofOfStake, ExportThreadCycleState, RollCounts, RollUpdate, RollUpdates};
pub use timeslots::{
    get_block_slot_timestamp, get_current_latest_block_slot, get_latest_block_slot_at_timestamp,
    time_range_to_slot_range,
};

mod block_graph;
mod config;
mod consensus_controller;
mod consensus_worker;
pub mod error;
pub mod ledger;
mod pos;
mod timeslots;

#[cfg(test)]
mod tests;
