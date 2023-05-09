use massa_signature::KeyPair;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate)  struct ConsensusConfig {
    /// Genesis timestamp
    pub(crate)  genesis_timestamp: MassaTime,
    /// Delta time between two period
    pub(crate)  t0: MassaTime,
    /// Number of threads
    pub(crate)  thread_count: u8,
    /// Keypair to sign genesis blocks.
    pub(crate)  genesis_key: KeyPair,
    /// Maximum number of blocks allowed in discarded blocks.
    pub(crate)  max_discarded_blocks: usize,
    /// If a block `is future_block_processing_max_periods` periods in the future, it is just discarded.
    pub(crate)  future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in `FutureIncomingBlocks`.
    pub(crate)  max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in `DependencyWaitingBlocks`.
    pub(crate)  max_dependency_blocks: usize,
    /// max event send wait
    pub(crate)  max_send_wait: MassaTime,
    /// old blocks are pruned every `block_db_prune_interval`
    pub(crate)  block_db_prune_interval: MassaTime,
    /// max number of items returned while querying
    pub(crate)  max_item_return_count: usize,
    /// Max gas per block for the execution configuration
    pub(crate)  max_gas_per_block: u64,
    /// Threshold for fitness.
    pub(crate)  delta_f0: u64,
    /// Maximum operation validity period count
    pub(crate)  operation_validity_periods: u64,
    /// cycle duration in periods
    pub(crate)  periods_per_cycle: u64,
    /// force keep at least this number of final periods in RAM for each thread
    pub(crate)  force_keep_final_periods: u64,
    /// target number of endorsement per block
    pub(crate)  endorsement_count: u32,
    /// TESTNET: time when the blockclique is ended.
    pub(crate)  end_timestamp: Option<MassaTime>,
    /// stats time span
    pub(crate)  stats_timespan: MassaTime,
    /// channel size
    pub(crate)  channel_size: usize,
    /// size of a consensus bootstrap streaming part
    pub(crate)  bootstrap_part_size: u64,
    /// whether broadcast is enabled
    pub(crate)  broadcast_enabled: bool,
    /// blocks headers channel capacity
    pub(crate)  broadcast_blocks_headers_channel_capacity: usize,
    /// blocks channel capacity
    pub(crate)  broadcast_blocks_channel_capacity: usize,
    /// filled blocks channel capacity
    pub(crate)  broadcast_filled_blocks_channel_capacity: usize,
    /// last start period
    pub(crate)  last_start_period: u64,
}
