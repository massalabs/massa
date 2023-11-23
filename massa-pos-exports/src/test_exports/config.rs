use massa_models::config::{
    MAX_DEFERRED_CREDITS_LENGTH, MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH,
    PERIODS_PER_CYCLE, POS_SAVED_CYCLES, THREAD_COUNT,
};

use crate::PoSConfig;

impl Default for PoSConfig {
    fn default() -> Self {
        Self {
            periods_per_cycle: PERIODS_PER_CYCLE,
            thread_count: THREAD_COUNT,
            cycle_history_length: POS_SAVED_CYCLES,
            max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
            max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
            max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
            initial_deferred_credits_path: None,
        }
    }
}
