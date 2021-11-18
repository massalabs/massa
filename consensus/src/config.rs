// Copyright (c) 2021 MASSA LABS <info@massa.net>

use models::{AlgoConfig, Amount};
use num::rational::Ratio;
use serde::{Deserialize, Serialize};
use signature::PrivateKey;
use std::{default::Default, path::PathBuf, usize};
use time::UTime;

pub const CHANNEL_SIZE: usize = 256;

/// Consensus configuration
/// Assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConsensusConfig {
    /// Time in millis when the blockclqieu started.
    pub genesis_timestamp: UTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<UTime>,
    /// Number of threds
    pub thread_count: u8,
    /// Time between the periods in the same thread.
    pub t0: UTime,
    /// Private_key to sign genesis blocks.
    pub genesis_key: PrivateKey,
    /// Staking private keys
    pub staking_keys_path: PathBuf,
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
    /// Maximum tries to fill a block with operations
    pub max_operations_fill_attempts: u32,
    /// Maximum block size in bytes
    pub max_block_size: u32,
    /// Maximum operation validity period count
    pub operation_validity_periods: u64,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// PoS lookback cycles: when drawing for cycle N, we use the rolls from cycle N - pos_lookback_cycles - 1
    pub pos_lookback_cycles: u64,
    /// PoS lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting  pos_lock_cycles
    pub pos_lock_cycles: u64,
    // number of cached draw cycles for PoS
    pub pos_draw_cached_cycles: usize,
    // number of cycle misses (strictly) above which stakers are deactivated
    pub pos_miss_rate_deactivation_threshold: Ratio<u64>,
    /// path to ledger db
    pub ledger_path: PathBuf,
    pub ledger_cache_capacity: u64,
    pub ledger_flush_interval: Option<UTime>,
    pub ledger_reset_at_startup: bool,
    pub initial_ledger_path: PathBuf,
    pub block_reward: Amount,
    pub operation_batch_size: usize,
    pub initial_rolls_path: PathBuf,
    pub initial_draw_seed: String,
    pub roll_price: Amount,
    // stats timespan
    pub stats_timespan: UTime,
    // max event send wait
    pub max_send_wait: UTime,
    pub endorsement_count: u32,
    pub block_db_prune_interval: UTime,
    pub max_item_return_count: usize,

    /// If we want to generate blocks.
    /// Parameter that shouldn't be defined in prod.
    #[serde(skip, default = "Default::default")]
    pub disable_block_creation: bool,
}

impl ConsensusConfig {
    pub fn to_algo_config(&self) -> AlgoConfig {
        AlgoConfig {
            genesis_timestamp: self.genesis_timestamp,
            end_timestamp: self.end_timestamp,
            thread_count: self.thread_count,
            t0: self.t0,
            delta_f0: self.delta_f0,
            operation_validity_periods: self.operation_validity_periods,
            periods_per_cycle: self.periods_per_cycle,
            pos_lookback_cycles: self.pos_lookback_cycles,
            pos_lock_cycles: self.pos_lock_cycles,
            block_reward: self.block_reward,
            roll_price: self.roll_price,
        }
    }
}
