//! Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This file defines a configuration structure containing all settings for final state management

use massa_async_pool::AsyncPoolConfig;
use massa_executed_ops::{ExecutedDenunciationsConfig, ExecutedOpsConfig};
use massa_ledger_exports::LedgerConfig;
use massa_pos_exports::PoSConfig;
use massa_time::MassaTime;
use std::path::PathBuf;

/// Ledger configuration
#[derive(Debug, Clone)]
pub struct FinalStateConfig {
    /// ledger configuration
    pub ledger_config: LedgerConfig,
    /// asynchronous pool configuration
    pub async_pool_config: AsyncPoolConfig,
    /// proof-of-stake configuration
    pub pos_config: PoSConfig,
    /// executed operations configuration
    pub executed_ops_config: ExecutedOpsConfig,
    /// executed denunciations configuration
    pub executed_denunciations_config: ExecutedDenunciationsConfig,
    /// final changes history length
    pub final_history_length: usize,
    /// thread count
    pub thread_count: u8,
    /// periods per cycle
    pub periods_per_cycle: u64,
    /// initial PoS seed string
    pub initial_seed_string: String,
    /// initial rolls file path
    pub initial_rolls_path: PathBuf,
    /// endorsement count
    pub endorsement_count: u32,
    /// max number of denunciation index in executed denunciations struct
    pub max_executed_denunciations_length: u64,
    /// max number of denunciations that can be included in a block header
    /// or in executed denunciations struct
    pub max_denunciations_per_block_header: u32,
    /// TODO
    pub t0: MassaTime,
    /// TODO
    pub genesis_timestamp: MassaTime,
}
