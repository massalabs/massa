// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![feature(async_closure)]
#![feature(bool_to_option)]
#![feature(hash_drain_filter)]
#![feature(map_first_last)]

#[macro_use]
extern crate logging;

mod block_graph;
mod config;
mod consensus_controller;
mod consensus_worker;
mod error;
mod ledger;
mod pos;
mod timeslots;

pub use block_graph::BootstrapableGraph;
pub use block_graph::{
    BlockGraphExport, Clique, DiscardReason, ExportActiveBlock, ExportBlockStatus, ExportClique,
    ExportCompiledBlock, ExportDiscardedBlocks, LedgerDataExport, Status,
};
pub use config::ConsensusConfig;
pub use consensus_controller::{
    start_consensus_controller, ConsensusCommandSender, ConsensusEventReceiver, ConsensusManager,
};
pub use consensus_worker::{AddressState, ConsensusCommand, ConsensusEvent, ConsensusStats};
pub use error::ConsensusError;
pub use ledger::{LedgerChange, LedgerData, LedgerExport};
pub use pos::{ExportProofOfStake, ExportThreadCycleState, RollCounts, RollUpdate, RollUpdates};
pub use timeslots::{
    get_block_slot_timestamp, get_current_latest_block_slot, get_latest_block_slot_at_timestamp,
};

#[cfg(test)]
mod tests;
