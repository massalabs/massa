use massa_models::config::{
    constants::{
        CHANNEL_SIZE, DELTA_F0, ENDORSEMENT_COUNT, GENESIS_KEY, GENESIS_TIMESTAMP,
        MAX_GAS_PER_BLOCK, OPERATION_VALIDITY_PERIODS, PERIODS_PER_CYCLE, T0, THREAD_COUNT,
    },
    CONSENSUS_BOOTSTRAP_PART_SIZE,
};
use massa_time::MassaTime;

use crate::ConsensusConfig;

impl Default for ConsensusConfig {
    fn default() -> Self {
        Self {
            genesis_timestamp: *GENESIS_TIMESTAMP,
            t0: T0,
            thread_count: THREAD_COUNT,
            genesis_key: GENESIS_KEY.clone(),
            max_discarded_blocks: 10000,
            max_future_processing_blocks: 100,
            max_dependency_blocks: 2048,
            block_db_prune_interval: MassaTime::from_millis(5000),
            max_gas_per_block: MAX_GAS_PER_BLOCK,
            delta_f0: DELTA_F0,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            force_keep_final_periods: 20,
            force_keep_final_periods_without_ops: 128,
            endorsement_count: ENDORSEMENT_COUNT,
            end_timestamp: None,
            stats_timespan: MassaTime::from_millis(60000),
            channel_size: CHANNEL_SIZE,
            bootstrap_part_size: CONSENSUS_BOOTSTRAP_PART_SIZE,
            broadcast_enabled: true,
            broadcast_blocks_headers_channel_capacity: 128,
            broadcast_blocks_channel_capacity: 128,
            broadcast_filled_blocks_channel_capacity: 128,
            last_start_period: 0,
        }
    }
}
