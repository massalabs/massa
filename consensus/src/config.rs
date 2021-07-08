use crypto::signature::{PrivateKey, PublicKey};
use serde::Deserialize;
use std::default::Default;
use time::UTime;

pub const CHANNEL_SIZE: usize = 16;

/// Consensus configuration
/// Assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
#[derive(Debug, Deserialize, Clone)]
pub struct ConsensusConfig {
    /// Time in millis when the blockclqieu started.
    pub genesis_timestamp: UTime,
    /// Number of threds
    pub thread_count: u8,
    /// Time between the periods in the same slot.
    pub t0: UTime,
    /// Initial seed of the random selector.
    pub selection_rng_seed: u64,
    /// Private_key to sign genesis blocks.
    pub genesis_key: PrivateKey,
    /// List of key for every node in the network.
    pub nodes: Vec<(PublicKey, PrivateKey)>,
    /// Index of our node in the previous list.
    pub current_node_index: u32,
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// If a block  is future_block_processing_max_periods periods in the future, it is just discarded.
    pub future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in FutureIncomingBlocks.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in DependencyWaitingBlocks.
    pub max_dependency_blocks: usize,
    /// Threshold for fitness.
    pub delta_f0: u64,
    /// Maximum number of operations per block
    pub max_operations_per_block: u32,
    /// Maximum block size in bytes
    pub max_block_size: u32,

    /// If we want to generate blocks.
    /// Parameter that shouldn't be defined in prod.
    #[serde(skip, default = "Default::default")]
    pub disable_block_creation: bool,
}
