use crate::FactoryConfig;
use massa_time::MassaTime;

impl Default for FactoryConfig {
    fn default() -> Self {
        use massa_models::constants::default_testing::*;

        FactoryConfig {
            thread_count: THREAD_COUNT,
            genesis_timestamp: MassaTime::now().expect("failed to get current time"),
            t0: T0,
            clock_compensation_millis: 0,
            initial_delay: MassaTime::from(0),
            max_block_size: MAX_BLOCK_SIZE as u64,
            max_block_gas: MAX_GAS_PER_BLOCK,
        }
    }
}
