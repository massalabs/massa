// Copyright (c) 2021 MASSA LABS <info@massa.net>

use std::fmt::Display;

use crate::{Amount, thread_count};
use massa_time::MassaTime;
use serde::{Deserialize, Serialize};

/// Compact representation of key values of consensus algorithm used in API
#[derive(Debug, Deserialize, Serialize, Clone, Copy)]
pub struct CompactConfig {
    /// Time in millis when the blockclique started.
    pub genesis_timestamp: MassaTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<MassaTime>,
    /// Time between the periods in the same thread.
    pub t0: MassaTime,
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

impl Display for CompactConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "    Genesis timestamp: {}",
            self.genesis_timestamp.to_utc_string()
        )?;
        if let Some(end) = self.end_timestamp {
            writeln!(f, "    End timestamp: {}", end.to_utc_string())?;
        }
        writeln!(f, "    Thread count: {}", thread_count())?;
        writeln!(f, "    t0: {}", self.t0)?;
        writeln!(f, "    delta_f0: {}", self.delta_f0)?;
        writeln!(
            f,
            "    Operation validity periods: {}",
            self.operation_validity_periods
        )?;
        writeln!(f, "    Periods per cycle: {}", self.periods_per_cycle)?;
        writeln!(
            f,
            "    Proof of stake lookback cycles: {}",
            self.pos_lookback_cycles
        )?;
        writeln!(
            f,
            "    Proof of stake lock cycles: {}",
            self.pos_lock_cycles
        )?;
        writeln!(f, "    Block reward: {}", self.block_reward)?;
        writeln!(f, "    Periods per cycle: {}", self.periods_per_cycle)?;
        Ok(())
    }
}
