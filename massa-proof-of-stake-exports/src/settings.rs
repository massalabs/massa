use std::path::PathBuf;

use massa_models::Amount;
use massa_signature::PrivateKey;
use num::rational::Ratio;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProofOfStakeConfig {
    /// Number of threads
    pub thread_count: u8,
    /// Private_key to sign genesis blocks.
    pub genesis_key: PrivateKey,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// PoS lookback cycles: when drawing for cycle N, we use the rolls from cycle N - pos_lookback_cycles - 1
    pub pos_lookback_cycles: u64,
    /// PoS lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting  pos_lock_cycles
    pub pos_lock_cycles: u64,
    /// number of cached draw cycles for PoS
    pub pos_draw_cached_cycles: usize,
    /// number of cycle misses (strictly) above which stakers are deactivated
    pub pos_miss_rate_deactivation_threshold: Ratio<u64>,
    pub initial_rolls_path: PathBuf,
    pub initial_draw_seed: String,
    pub roll_price: Amount,
    pub endorsement_count: u32,
}
