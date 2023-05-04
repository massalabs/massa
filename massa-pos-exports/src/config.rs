// Copyright (c) 2022 MASSA LABS <info@massa.net>

/// proof-of-stake final state configuration
#[derive(Debug, Clone)]
pub struct PoSConfig {
    /// periods per cycle
    pub periods_per_cycle: u64,
    /// thread count
    pub thread_count: u8,
    /// number of saved cycle
    pub cycle_history_length: usize,
    /// maximum size of a deferred credits bootstrap part
    pub credits_bootstrap_part_size: u64,
    /// maximum rolls length
    pub max_rolls_length: u64,
    /// maximum production stats length
    pub max_production_stats_length: u64,
    /// maximum deferred credits length
    pub max_credit_length: u64,
}
