use std::path::PathBuf;

use massa_signature::{PrivateKey, PRIVATE_KEY_SIZE_BYTES};
use serde::{Deserialize, Serialize};

/// Configuration of selector thread
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SelectorConfig {
    /// Number of running threads
    pub thread_count: u8,
    /// Nuber of blocks in a cycle
    pub blocks_in_cycle: usize,
    /// Number of endorsement
    pub endorsement_count: usize,
    /// Maximum number of computed draws by cycle we keep in cache
    pub max_draw_cache: usize,
    /// Loopback cycles
    pub lookback_cycles: usize,
    /// Number of periods per cycle
    pub periods_per_cycle: u64,
    /// `PrivateKey` to sign genesis blocks.
    pub genesis_key: PrivateKey,
    /// path to initial rolls
    pub initial_rolls_path: PathBuf,
    /// initial seed
    pub initial_draw_seed: String,
}

impl Default for SelectorConfig {
    fn default() -> Self {
        Self {
            thread_count: 0,
            blocks_in_cycle: 0,
            endorsement_count: 0,
            max_draw_cache: 0,
            lookback_cycles: 0,
            periods_per_cycle: 0,
            genesis_key: PrivateKey::from_bytes(&[0u8; PRIVATE_KEY_SIZE_BYTES]).unwrap(),
            initial_rolls_path: PathBuf::default(),
            initial_draw_seed: String::default(),
        }
    }
}
