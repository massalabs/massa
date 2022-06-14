//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use crate::{FinalState, FinalStateConfig};
use massa_async_pool::{AsyncPool, AsyncPoolConfig};
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_models::Slot;

/// Default value of `FinalState` used for tests
impl Default for FinalState {
    fn default() -> Self {
        let config = FinalStateConfig::default();
        let slot = Slot::new(0, config.thread_count.saturating_sub(1));

        // load the initial final ledger from file
        let ledger = FinalLedger::default();

        // create the async pool
        let async_pool = AsyncPool::new(config.async_pool_config.clone());

        // generate the final state
        FinalState {
            slot,
            ledger: Box::new(ledger),
            async_pool,
            config,
            changes_history: Default::default(), // no changes in history
        }
    }
}

/// Default value of `FinalStateConfig` used for tests
impl Default for FinalStateConfig {
    fn default() -> Self {
        FinalStateConfig {
            ledger_config: LedgerConfig::default(),
            async_pool_config: AsyncPoolConfig::default(),
            final_history_length: 10,
            thread_count: 2,
        }
    }
}
