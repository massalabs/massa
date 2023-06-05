use massa_signature::KeyPair;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ConsensusConfig {
    /// Genesis timestamp
    pub genesis_timestamp: MassaTime,
    /// Delta time between two period
    pub t0: MassaTime,
    /// Number of threads
    pub thread_count: u8,
    /// Keypair to sign genesis blocks.
    pub genesis_key: KeyPair,
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// Maximum number of blocks allowed in `FutureIncomingBlocks`.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in `DependencyWaitingBlocks`.
    pub max_dependency_blocks: usize,
    /// old blocks are pruned every `block_db_prune_interval`
    pub block_db_prune_interval: MassaTime,
    /// Max gas per block for the execution configuration
    pub max_gas_per_block: u64,
    /// Threshold for fitness.
    pub delta_f0: u64,
    /// Maximum operation validity period count
    pub operation_validity_periods: u64,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    /// force keep at least this number of final blocks without ops in RAM for each thread
    pub force_keep_final_periods_without_ops: u64,
    /// target number of endorsement per block
    pub endorsement_count: u32,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<MassaTime>,
    /// stats time span
    pub stats_timespan: MassaTime,
    /// channel size
    pub channel_size: usize,
    /// size of a consensus bootstrap streaming part
    pub bootstrap_part_size: u64,
    /// whether broadcast is enabled
    pub broadcast_enabled: bool,
    /// blocks headers channel capacity
    pub broadcast_blocks_headers_channel_capacity: usize,
    /// blocks channel capacity
    pub broadcast_blocks_channel_capacity: usize,
    /// filled blocks channel capacity
    pub broadcast_filled_blocks_channel_capacity: usize,
    /// last start period
    pub last_start_period: u64,
}
