//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for final state management

use massa_async_pool::AsyncPoolConfig;
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_ledger_exports::LedgerConfig;
use massa_pos_exports::PoSConfig;
use std::path::PathBuf;

/// Ledger configuration
#[derive(Debug, Clone)]
pub(crate) struct FinalStateConfig {
    /// ledger configuration
    pub(crate) ledger_config: LedgerConfig,
    /// asynchronous pool configuration
    pub(crate) async_pool_config: AsyncPoolConfig,
    /// proof-of-stake configuration
    pub(crate) pos_config: PoSConfig,
    /// executed operations configuration
    pub(crate) executed_ops_config: ExecutedOpsConfig,
    /// executed denunciations configuration
    pub(crate) executed_denunciations_config: ExecutedDenunciationsConfig,
    /// final changes history length
    pub(crate) final_history_length: usize,
    /// thread count
    pub(crate) thread_count: u8,
    /// periods per cycle
    pub(crate) periods_per_cycle: u64,
    /// initial PoS seed string
    pub(crate) initial_seed_string: String,
    /// initial rolls file path
    pub(crate) initial_rolls_path: PathBuf,
    /// endorsement count
    pub(crate) endorsement_count: u32,
    /// max number of denunciation index in executed denunciations struct
    pub(crate) max_executed_denunciations_length: u64,
    /// max number of denunciations that can be included in a block header
    /// or in executed denunciations struct
    pub(crate) max_denunciations_per_block_header: u32,
}
