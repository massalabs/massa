use super::*;
use crate::amount::Amount;
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, num::NonZeroU8};

/// Compact representation of key values of consensus algorithm used in API
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct CompactConfig {
    /// Time in milliseconds when the blockclique started.
    pub genesis_timestamp: MassaTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<MassaTime>,
    /// Number of threads
    pub thread_count: NonZeroU8,
    /// Time between the periods in the same thread.
    pub t0: MassaTime,
    /// Threshold for fitness.
    pub delta_f0: u64,
    /// Maximum operation validity period count
    pub operation_validity_periods: u64,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// Reward amount for a block creation
    pub block_reward: Amount,
    /// Price of a roll on the network
    pub roll_price: Amount,
    /// Max total size of a block
    pub max_block_size: u32,
}

impl Default for CompactConfig {
    fn default() -> Self {
        Self {
            genesis_timestamp: *GENESIS_TIMESTAMP,
            end_timestamp: *END_TIMESTAMP,
            thread_count: THREAD_COUNT,
            t0: T0,
            delta_f0: DELTA_F0,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            block_reward: BLOCK_REWARD,
            roll_price: ROLL_PRICE,
            max_block_size: MAX_BLOCK_SIZE,
        }
    }
}

impl Display for CompactConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "    Genesis time: {}",
            self.genesis_timestamp.to_utc_string()
        )?;
        if let Some(end) = self.end_timestamp {
            writeln!(f, "    End time: {}", end.to_utc_string())?;
        }
        writeln!(f, "    Thread count: {}", self.thread_count)?;
        writeln!(f, "    t0: {}", self.t0)?;
        writeln!(f, "    delta_f0: {}", self.delta_f0)?;
        writeln!(
            f,
            "    Operation validity periods: {}",
            self.operation_validity_periods
        )?;
        writeln!(f, "    Periods per cycle: {}", self.periods_per_cycle)?;
        writeln!(f, "    Block reward: {}", self.block_reward)?;
        writeln!(f, "    Periods per cycle: {}", self.periods_per_cycle)?;
        writeln!(f, "    Roll price: {}", self.roll_price)?;
        writeln!(f, "    Max block size (in bytes): {}", self.max_block_size)?;
        Ok(())
    }
}
