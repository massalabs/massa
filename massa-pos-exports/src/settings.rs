use std::path::PathBuf;

use massa_models::constants::{
    ENDORSEMENT_COUNT, PERIODS_PER_CYCLE, POS_LOOKBACK_CYCLES, THREAD_COUNT,
};
use massa_signature::{KeyPair, SECRET_KEY_BYTES_SIZE};
use serde::{Deserialize, Serialize};

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

impl Default for SelectorConfig {
    fn default() -> Self {
        #[cfg(not(feature = "sandbox"))]
        let thread_count = THREAD_COUNT;
        #[cfg(feature = "sandbox")]
        let thread_count = *THREAD_COUNT;

        Self {
            thread_count,
            blocks_in_cycle: thread_count as usize * PERIODS_PER_CYCLE as usize,
            endorsement_count: ENDORSEMENT_COUNT,
            max_draw_cache: 0,
            lookback_cycles: POS_LOOKBACK_CYCLES,
            periods_per_cycle: PERIODS_PER_CYCLE,
            genesis_key: KeyPair::from_bytes(&[0u8; SECRET_KEY_BYTES_SIZE]).unwrap(),
            initial_rolls_path: PathBuf::default(),
            initial_draw_seed: String::default(),
        }
    }
}
