// Copyright (c) 2021 MASSA LABS <info@massa.net>

#![allow(clippy::assertions_on_constants)]

use massa_models::{Amount, CompactConfig};
#[cfg(test)]
use massa_signature::generate_random_private_key;
use massa_signature::PrivateKey;
use massa_time::MassaTime;
use num::rational::Ratio;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::{default::Default, path::PathBuf, usize};

// Consensus static parameters (defined by protocol used)
// Changing one of the following values is considered as a breaking change
// Values differ in `test` flavor building for faster CI and simpler scenarios
pub const CHANNEL_SIZE: usize = 256;

#[cfg(not(test))]
lazy_static::lazy_static! {
    /// Time in millis when the blockclique started.
    pub static ref GENESIS_TIMESTAMP: MassaTime = if cfg!(feature = "test") {
        MassaTime::now().unwrap().saturating_add(MassaTime::from(1000 * 60 * 3))
    } else {
        1641754800000.into()
    };

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = if cfg!(feature = "test") {None} else {Some(1643644800000.into())};

    /// Time between the periods in the same thread.
    pub static ref T0: MassaTime = 16000.into();

    /// Private_key to sign genesis blocks.
    pub static ref GENESIS_KEY: PrivateKey = "SGoTK5TJ9ZcCgQVmdfma88UdhS6GK94aFEYAsU3F1inFayQ6S"
        .parse()
        .unwrap();

    pub static ref BLOCK_REWARD: Amount = Amount::from_str("0.3").unwrap();

    pub static ref INITIAL_DRAW_SEED: String = "massa_genesis_seed".to_string();

    /// number of cycle misses (strictly) above which stakers are deactivated
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(7, 10);

    pub static ref ROLL_PRICE: Amount = Amount::from_str("100.0").unwrap();
}

#[cfg(test)]
lazy_static::lazy_static! {
    /// Time in millis when the blockclique started.
    pub static ref GENESIS_TIMESTAMP: MassaTime = MassaTime::now().unwrap();

    /// TESTNET: time when the blockclique is ended.
    pub static ref END_TIMESTAMP: Option<MassaTime> = None;

    /// Time between the periods in the same thread.
    pub static ref T0: MassaTime = 32000.into();

    /// Private_key to sign genesis blocks.
    pub static ref GENESIS_KEY: PrivateKey = generate_random_private_key();

    pub static ref BLOCK_REWARD: Amount = Amount::from_str("1.0").unwrap();

    pub static ref INITIAL_DRAW_SEED: String = "massa_genesis_seed".to_string();

    /// number of cycle misses (strictly) above which stakers are deactivated
    pub static ref POS_MISS_RATE_DEACTIVATION_THRESHOLD: Ratio<u64> = Ratio::new(1, 1);

    pub static ref ROLL_PRICE: Amount = Amount::from_str("100.0").unwrap();
}

/// Number of threads
#[cfg(not(test))]
pub const THREAD_COUNT: u8 = 32;
#[cfg(test)]
const THREAD_COUNT: u8 = 2;

#[cfg(not(test))]
pub const ENDORSEMENT_COUNT: u32 = 9;
#[cfg(test)]
const ENDORSEMENT_COUNT: u32 = 0;

/// Threshold for fitness.
#[cfg(not(test))]
pub const DELTA_F0: u64 = 640;
#[cfg(test)]
const DELTA_F0: u64 = 32;

/// Maximum number of operations per block
#[cfg(not(test))]
pub const MAX_OPERATIONS_PER_BLOCK: u32 = 102400;
#[cfg(test)]
const MAX_OPERATIONS_PER_BLOCK: u32 = 1024;

/// Maximum block size in bytes
#[cfg(not(test))]
pub const MAX_BLOCK_SIZE: u32 = 102400;
#[cfg(test)]
const MAX_BLOCK_SIZE: u32 = 3145728;

/// Maximum operation validity period count
#[cfg(not(test))]
pub const OPERATION_VALIDITY_PERIODS: u64 = 10;
#[cfg(test)]
const OPERATION_VALIDITY_PERIODS: u64 = 1;

/// cycle duration in periods
#[cfg(not(test))]
pub const PERIODS_PER_CYCLE: u64 = 128;
#[cfg(test)]
const PERIODS_PER_CYCLE: u64 = 100;

/// PoS lookback cycles: when drawing for cycle N, we use the rolls from cycle N - pos_lookback_cycles - 1
#[cfg(not(test))]
pub const POS_LOOKBACK_CYCLES: u64 = 2;
#[cfg(test)]
const POS_LOOKBACK_CYCLES: u64 = 2;

/// PoS lock cycles: when some rolls are released, we only credit the coins back to their owner after waiting  pos_lock_cycles
#[cfg(not(test))]
pub const POS_LOCK_CYCLES: u64 = 1;
#[cfg(test)]
const POS_LOCK_CYCLES: u64 = 1;

pub const MAX_GAS_PER_BLOCK: u64 = 100_000_000;

/// Consensus configuration
/// Assumes thread_count >= 1, t0_millis >= 1, t0_millis % thread_count == 0
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConsensusSettings {
    /// Staking private keys
    pub staking_keys_path: PathBuf,
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// If a block  is future_block_processing_max_periods periods in the future, it is just discarded.
    pub future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in FutureIncomingBlocks.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in DependencyWaitingBlocks.
    pub max_dependency_blocks: usize,
    /// Maximum tries to fill a block with operations
    pub max_operations_fill_attempts: u32,
    // number of cached draw cycles for PoS
    pub pos_draw_cached_cycles: usize,
    /// path to ledger db
    pub ledger_path: PathBuf,
    pub ledger_cache_capacity: u64,
    pub ledger_flush_interval: Option<MassaTime>,
    pub ledger_reset_at_startup: bool,
    pub initial_ledger_path: PathBuf,
    pub operation_batch_size: usize,
    pub initial_rolls_path: PathBuf,
    // stats timespan
    pub stats_timespan: MassaTime,
    // max event send wait
    pub max_send_wait: MassaTime,
    // force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    pub block_db_prune_interval: MassaTime,
    pub max_item_return_count: usize,
    /// If we want to generate blocks.
    /// Parameter that shouldn't be defined in prod.
    #[serde(skip, default = "Default::default")]
    pub disable_block_creation: bool,
}

impl ConsensusSettings {
    /// Utility method to derivate a full configuration (static + user defined) from an user defined one
    /// This is really handy in tests where we want to mutate static defined values
    pub fn config(&self) -> ConsensusConfig {
        // TODO: these assertion should be checked at compile time
        // https://github.com/rust-lang/rfcs/issues/2790
        assert!(THREAD_COUNT >= 1);
        assert!((*T0).to_millis() >= 1);
        assert!((*T0).to_millis() % (THREAD_COUNT as u64) == 0);
        ConsensusConfig {
            genesis_timestamp: *GENESIS_TIMESTAMP,
            end_timestamp: *END_TIMESTAMP,
            thread_count: THREAD_COUNT,
            t0: *T0,
            genesis_key: *GENESIS_KEY,
            staking_keys_path: self.staking_keys_path.clone(),
            max_discarded_blocks: self.max_discarded_blocks,
            future_block_processing_max_periods: self.future_block_processing_max_periods,
            max_future_processing_blocks: self.max_future_processing_blocks,
            max_dependency_blocks: self.max_dependency_blocks,
            delta_f0: DELTA_F0,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            max_operations_fill_attempts: self.max_operations_fill_attempts,
            max_block_size: MAX_BLOCK_SIZE,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            pos_lookback_cycles: POS_LOOKBACK_CYCLES,
            pos_lock_cycles: POS_LOCK_CYCLES,
            pos_draw_cached_cycles: self.pos_draw_cached_cycles,
            pos_miss_rate_deactivation_threshold: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
            ledger_path: self.ledger_path.clone(),
            ledger_cache_capacity: self.ledger_cache_capacity,
            ledger_flush_interval: self.ledger_flush_interval,
            ledger_reset_at_startup: self.ledger_reset_at_startup,
            initial_ledger_path: self.initial_ledger_path.clone(),
            block_reward: *BLOCK_REWARD,
            operation_batch_size: self.operation_batch_size,
            initial_rolls_path: self.initial_rolls_path.clone(),
            initial_draw_seed: INITIAL_DRAW_SEED.clone(),
            roll_price: *ROLL_PRICE,
            stats_timespan: self.stats_timespan,
            max_send_wait: self.max_send_wait,
            force_keep_final_periods: self.force_keep_final_periods,
            endorsement_count: ENDORSEMENT_COUNT,
            block_db_prune_interval: self.block_db_prune_interval,
            max_item_return_count: self.max_item_return_count,
            disable_block_creation: self.disable_block_creation,
        }
    }
}

/// Consensus full configuration (static + user defined)
///
/// Assert that `THREAD_COUNT >= 1 || T0.to_millis() >= 1 || T0.to_millis() % THREAD_COUNT == 0`
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ConsensusConfig {
    /// Time in millis when the blockclique started.
    pub genesis_timestamp: MassaTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<MassaTime>,
    /// Number of threads
    pub thread_count: u8,
    /// Time between the periods in the same thread.
    pub t0: MassaTime,
    /// Private_key to sign genesis blocks.
    pub genesis_key: PrivateKey,
    /// Staking private keys
    pub staking_keys_path: PathBuf,
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// If a block  is future_block_processing_max_periods periods in the future, it is just discarded.
    pub future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in FutureIncomingBlocks.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in DependencyWaitingBlocks.
    pub max_dependency_blocks: usize,
    /// Threshold for fitness.
    pub delta_f0: u64,
    /// Maximum number of operations per block
    pub max_operations_per_block: u32,
    /// Maximum tries to fill a block with operations
    pub max_operations_fill_attempts: u32,
    /// Maximum block size in bytes
    pub max_block_size: u32,
    /// Maximum operation validity period count
    pub operation_validity_periods: u64,
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
    /// path to ledger db
    pub ledger_path: PathBuf,
    pub ledger_cache_capacity: u64,
    pub ledger_flush_interval: Option<MassaTime>,
    pub ledger_reset_at_startup: bool,
    pub initial_ledger_path: PathBuf,
    pub block_reward: Amount,
    pub operation_batch_size: usize,
    pub initial_rolls_path: PathBuf,
    pub initial_draw_seed: String,
    pub roll_price: Amount,
    /// stats timespan
    pub stats_timespan: MassaTime,
    /// max event send wait
    pub max_send_wait: MassaTime,
    /// force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    pub endorsement_count: u32,
    pub block_db_prune_interval: MassaTime,
    pub max_item_return_count: usize,
    /// If we want to generate blocks.
    /// Parameter that shouldn't be defined in prod.
    #[serde(skip, default = "Default::default")]
    pub disable_block_creation: bool,
}

lazy_static::lazy_static! {
    /// Compact representation of key values of consensus algorithm used in API
    static ref STATIC_CONFIG: CompactConfig = CompactConfig {
        genesis_timestamp: *GENESIS_TIMESTAMP,
        end_timestamp: *END_TIMESTAMP,
        thread_count: THREAD_COUNT,
        t0: *T0,
        delta_f0: DELTA_F0,
        operation_validity_periods: OPERATION_VALIDITY_PERIODS,
        periods_per_cycle: PERIODS_PER_CYCLE,
        pos_lookback_cycles: POS_LOOKBACK_CYCLES,
        pos_lock_cycles: POS_LOCK_CYCLES,
        block_reward: *BLOCK_REWARD,
        roll_price: *ROLL_PRICE,
    };
}

impl ConsensusConfig {
    /// Utility method to derivate a compact configuration (for API use) from a full one
    pub fn compact_config(&self) -> CompactConfig {
        *STATIC_CONFIG
    }
}

#[cfg(test)]
mod tests {
    use super::ConsensusSettings;
    use crate::ConsensusConfig;
    use massa_models::{init_serialization_context, SerializationContext};
    use massa_signature::{generate_random_private_key, PrivateKey};
    use std::path::Path;
    use tempfile::NamedTempFile;

    const SERIALIZATION_CONTEXT: SerializationContext = SerializationContext {
        max_block_operations: 1024,
        parent_count: 2,
        max_peer_list_length: 128,
        max_message_size: 3 * 1024 * 1024,
        max_block_size: 3 * 1024 * 1024,
        max_bootstrap_blocks: 100,
        max_bootstrap_cliques: 100,
        max_bootstrap_deps: 100,
        max_bootstrap_children: 100,
        max_ask_blocks_per_message: 10,
        max_operations_per_message: 1024,
        max_endorsements_per_message: 1024,
        max_bootstrap_message_size: 100000000,
        max_bootstrap_pos_entries: 1000,
        max_bootstrap_pos_cycles: 5,
        max_block_endorsements: 8,
    };

    pub fn generate_staking_keys_file(staking_keys: &Vec<PrivateKey>) -> NamedTempFile {
        use std::io::prelude::*;
        let f = NamedTempFile::new().expect("cannot create temp file");
        serde_json::to_writer_pretty(f.as_file(), &staking_keys)
            .expect("unable to write ledger file");
        f.as_file()
            .seek(std::io::SeekFrom::Start(0))
            .expect("could not seek file");
        f
    }

    impl From<&Path> for ConsensusConfig {
        /// Utility trait used in tests to get a full consensus configuration from an initial ledger path
        fn from(initial_ledger_path: &Path) -> Self {
            let mut staking_keys = Vec::new();
            for _ in 0..2 {
                staking_keys.push(generate_random_private_key());
            }
            init_serialization_context(SERIALIZATION_CONTEXT);
            ConsensusSettings {
                staking_keys_path: generate_staking_keys_file(&staking_keys)
                    .path()
                    .to_path_buf(),
                max_discarded_blocks: 10,
                future_block_processing_max_periods: 3,
                max_future_processing_blocks: 10,
                max_dependency_blocks: 10,
                max_operations_fill_attempts: 6,
                pos_draw_cached_cycles: 2,
                ledger_path: tempfile::tempdir()
                    .expect("cannot create temp dir")
                    .path()
                    .to_path_buf(),
                ledger_cache_capacity: 1000000,
                ledger_flush_interval: Some(200.into()),
                ledger_reset_at_startup: true,
                initial_ledger_path: initial_ledger_path.to_path_buf(),
                operation_batch_size: 100,
                initial_rolls_path: tempfile::tempdir()
                    .expect("cannot create temp dir")
                    .path()
                    .to_path_buf(),
                stats_timespan: 60000.into(),
                max_send_wait: 500.into(),
                force_keep_final_periods: 0,
                block_db_prune_interval: 1000.into(),
                max_item_return_count: 1000,
                disable_block_creation: true,
            }
            .config()
        }
    }
}
