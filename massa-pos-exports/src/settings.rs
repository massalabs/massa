use massa_signature::KeyPair;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration of selector thread
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SelectorConfig {
    /// Number of running threads
    pub thread_count: u8,
    /// Nuber of blocks in a cycle
    pub blocks_in_cycle: usize,
    /// Number of endorsement
    pub endorsement_count: u32,
    /// Maximum number of computed cycle's draws we keep in cache
    pub max_draw_cache: usize,
    /// Loopback cycles
    pub lookback_cycles: u64,
    /// Number of periods per cycle
    pub periods_per_cycle: u64,
    /// `KeyPair` to sign genesis blocks.
    pub genesis_key: KeyPair,
    /// path to initial rolls
    pub initial_rolls_path: PathBuf,
    /// initial seed
    pub initial_draw_seed: String,
}
