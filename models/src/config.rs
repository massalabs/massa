// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::Amount;
use serde::{Deserialize, Serialize};
use time::UTime;

/// Algo configuration
/// Assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlgoConfig {
    /// Time in millis when the blockclqieu started.
    pub genesis_timestamp: UTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<UTime>,
    /// Number of threds
    pub thread_count: u8,
    /// Time between the periods in the same thread.
    pub t0: UTime,
    /// Threshold for fitness.
    pub delta_f0: u64,
    /// Maximum operation validity period count
    pub operation_validity_periods: u64,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// PoS lookback cycles: when drawing for cycle N, we use the rolls from cycle N - pos_lookback_cycles - 1
    pub pos_lookback_cycles: u64,
    /// PoS lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting  pos_lock_cycles
    pub pos_lock_cycles: u64,
    pub block_reward: Amount,
    pub roll_price: Amount,
}
