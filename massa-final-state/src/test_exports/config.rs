//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines testing tools related to the configuration

use std::path::PathBuf;

use num::rational::Ratio;

use crate::{FinalState, FinalStateConfig};
use massa_async_pool::{AsyncPool, AsyncPoolConfig};
use massa_db_exports::ShareableMassaDBController;
use massa_deferred_calls::DeferredCallRegistry;
use massa_executed_ops::{
    ExecutedDenunciations, ExecutedDenunciationsConfig, ExecutedOps, ExecutedOpsConfig,
};
use massa_ledger_exports::LedgerConfig;
use massa_ledger_worker::FinalLedger;
use massa_models::config::{
    DENUNCIATION_EXPIRE_PERIODS, ENDORSEMENT_COUNT, GENESIS_TIMESTAMP,
    KEEP_EXECUTED_HISTORY_EXTRA_PERIODS, MAX_DEFERRED_CREDITS_LENGTH,
    MAX_DENUNCIATIONS_PER_BLOCK_HEADER, MAX_DENUNCIATION_CHANGES_LENGTH,
    MAX_PRODUCTION_STATS_LENGTH, MAX_ROLLS_COUNT_LENGTH, T0,
};
use massa_models::config::{PERIODS_PER_CYCLE, POS_SAVED_CYCLES, THREAD_COUNT};
use massa_pos_exports::{PoSConfig, PoSFinalState};
use massa_versioning::versioning::{MipStatsConfig, MipStore};

impl FinalState {
    /// Create a final state
    pub fn create_final_state(
        pos_state: PoSFinalState,
        config: FinalStateConfig,
        db: ShareableMassaDBController,
    ) -> Self {
        FinalState {
            ledger: Box::new(FinalLedger::new(config.ledger_config.clone(), db.clone())),
            async_pool: AsyncPool::new(config.async_pool_config.clone(), db.clone()),
            deferred_call_registry: DeferredCallRegistry::new(db.clone()),
            pos_state,
            executed_ops: ExecutedOps::new(config.executed_ops_config.clone(), db.clone()),
            executed_denunciations: ExecutedDenunciations::new(
                config.executed_denunciations_config.clone(),
                db.clone(),
            ),
            mip_store: MipStore::try_from((
                [],
                MipStatsConfig {
                    block_count_considered: 10,
                    warn_announced_version_ratio: Ratio::new(30, 100),
                },
            ))
            .unwrap(),
            config,
            last_start_period: 0,
            last_slot_before_downtime: None,
            db,
        }
    }
}

/// Default value of `FinalStateConfig` used for tests
impl Default for FinalStateConfig {
    fn default() -> FinalStateConfig {
        FinalStateConfig {
            ledger_config: LedgerConfig::default(),
            async_pool_config: AsyncPoolConfig::default(),
            executed_ops_config: ExecutedOpsConfig {
                thread_count: THREAD_COUNT,
                keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
            },
            executed_denunciations_config: ExecutedDenunciationsConfig {
                denunciation_expire_periods: DENUNCIATION_EXPIRE_PERIODS,
                thread_count: THREAD_COUNT,
                endorsement_count: ENDORSEMENT_COUNT,
                keep_executed_history_extra_periods: KEEP_EXECUTED_HISTORY_EXTRA_PERIODS,
            },
            pos_config: PoSConfig {
                periods_per_cycle: PERIODS_PER_CYCLE,
                thread_count: THREAD_COUNT,
                cycle_history_length: POS_SAVED_CYCLES,
                max_rolls_length: MAX_ROLLS_COUNT_LENGTH,
                max_production_stats_length: MAX_PRODUCTION_STATS_LENGTH,
                max_credit_length: MAX_DEFERRED_CREDITS_LENGTH,
                initial_deferred_credits_path: None,
            },
            final_history_length: 10,
            thread_count: 2,
            periods_per_cycle: 100,
            initial_rolls_path: PathBuf::new(),
            endorsement_count: ENDORSEMENT_COUNT,
            max_executed_denunciations_length: MAX_DENUNCIATION_CHANGES_LENGTH,
            initial_seed_string: "".to_string(),
            max_denunciations_per_block_header: MAX_DENUNCIATIONS_PER_BLOCK_HEADER,
            t0: T0,
            genesis_timestamp: *GENESIS_TIMESTAMP,
            ledger_backup_periods_interval: 100,
        }
    }
}
