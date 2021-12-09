// Copyright (c) 2021 MASSA LABS <info@massa.net>

use crate::Amount;
use massa_hash::HASH_SIZE_BYTES;
use massa_time::UTime;
use serde::{Deserialize, Serialize};

pub const ADDRESS_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const AMOUNT_DECIMAL_FACTOR: u64 = 1_000_000_000;

pub const BLOCK_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const ENDORSEMENT_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const OPERATION_ID_SIZE_BYTES: usize = HASH_SIZE_BYTES;

pub const SLOT_KEY_SIZE: usize = 9;

/// Configuration of consensus algorithm
/// Assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct Config {
    /// Time in millis when the blockclique started.
    pub genesis_timestamp: UTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<UTime>,
    /// Number of threads
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
