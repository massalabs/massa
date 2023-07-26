use massa_models::address::Address;
use serde::{Deserialize, Serialize};

/// Configuration of selector thread
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SelectorConfig {
    /// Number of running threads
    pub thread_count: u8,
    /// Number of endorsement
    pub endorsement_count: u32,
    /// Maximum number of computed cycle's draws we keep in cache
    pub max_draw_cache: usize,
    /// Number of periods per cycle
    pub periods_per_cycle: u64,
    /// genesis address to force draw genesis creators
    pub genesis_address: Address,
    /// communication channel length
    pub channel_size: usize,
    /// last_start_period, to know if we may expect cache cycle discontinuity
    pub last_start_period: Option<u64>,
}
