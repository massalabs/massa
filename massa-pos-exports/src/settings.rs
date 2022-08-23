use massa_models::Address;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    /// path to initial rolls
    pub initial_rolls_path: PathBuf,
    /// initial seed
    pub initial_draw_seed: String,
    /// communication channel length
    pub channel_size: usize,
}
