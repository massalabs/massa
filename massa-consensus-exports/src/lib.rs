// Copyright (c) 2022 MASSA LABS <info@massa.net>
//! Definition and exports of the graph types and errors.

mod channels;
mod controller_trait;
mod settings;

pub mod block_graph_export;
pub mod block_status;
pub mod bootstrapable_graph;
pub mod error;
pub mod events;
pub mod export_active_block;

pub use channels::{ConsensusBroadcasts, ConsensusChannels};
pub use controller_trait::{ConsensusController, ConsensusManager};
pub use settings::ConsensusConfig;

#[cfg(feature = "test-exports")]
pub use controller_trait::MockConsensusController;

/// Test utils
#[cfg(feature = "test-exports")]
/// Exports related to tests as Mocks and configurations
pub mod test_exports;
