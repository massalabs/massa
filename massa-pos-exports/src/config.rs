// Copyright (c) 2022 MASSA LABS <info@massa.net>

/// proof-of-stake final state configuration
#[derive(Debug, Clone)]
pub struct PoSConfig {
    /// periods per cycle
    pub(crate) periods_per_cycle: u64,
    /// thread count
    pub(crate) thread_count: u8,
    /// number of saved cycle
    pub cycle_history_length: usize,
    /// maximum size of a deferred credits bootstrap part
    pub(crate) credits_bootstrap_part_size: u64,
}
