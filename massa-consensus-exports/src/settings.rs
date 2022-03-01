// Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::assertions_on_constants)]
//! Definition & Implementation of the consensus settings
//! -----------------------------------------------------
//!
//! # Configurations
//!
//! * setting: read from user settings file
//! * config: merge of settings and hardcoded configuration that shouldn't be
//!           modifyed by user.
//!
//! This file is allowed to use a lot of constants from `massa-models` as all
//! other files named `settings.rs` or `config.rs`.
//!
//! The `ConsensusSettings` is the most basic and complete configuration in the
//! node. You can get almost every configuration from that one.
//!
//! `From<ConsensusSettings> impl *`:
//! - `ConsensusConfig`: Create a config merging user settings andhardcoded values
//!                      (see `/massa-models/node_configuration/*)
//!
//! `From<&ConsensusConfig> impl *`:
//! - GraphConfig
//! - LedgerConfig
//! - ProofOfStakeConfig
//!
//! > Development note: We clone the values on getting a configuration from another.
//!
//! # Usage of constants
//!
//! The default configuration is loaded from the `massa-models` crate. You shouldn't
//! write an hardcoded value in the following file but create a new value in
//! `default.rs` and the testing default equivalent value in `default_testing.rs`. See
//! `/node_configuration/mod.rs` documentation in `massa-models` sources for more
//! information.
//!
//! # Channels
//!
//! The following file contains the definition of the Channels structures used in
//! the current module.
//!
//! # Testing feature
//!
//! In unit test your allowed to use the `testing` feature flag that will
//! use the default values from `/node_configuration/default_testing.rs` in the
//! `massa-models` crate sources.
use massa_execution_exports::ExecutionController;
use massa_graph::{settings::GraphConfig, LedgerConfig};
use massa_models::Amount;
use massa_pool::PoolCommandSender;
use massa_proof_of_stake_exports::ProofOfStakeConfig;
use massa_protocol_exports::{ProtocolCommandSender, ProtocolEventReceiver};
use massa_signature::PrivateKey;
use massa_time::MassaTime;
use num::rational::Ratio;
use serde::{Deserialize, Serialize};
use std::{default::Default, path::PathBuf, usize};
use tokio::sync::mpsc;

use crate::{
    commands::{ConsensusCommand, ConsensusManagementCommand},
    events::ConsensusEvent,
};

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

/// Consensus full configuration (static + user defined)
#[derive(Debug)]
pub struct ConsensusConfig {
    #[cfg(feature = "testing")]
    pub temp_files: TempFiles,
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
    /// path to ledger db after initialization (merge with `initial_ledger_path` on node start)
    pub ledger_path: PathBuf,
    /// Cache capacity allowed to the ledger
    pub ledger_cache_capacity: u64,
    pub ledger_flush_interval: Option<MassaTime>,
    pub ledger_reset_at_startup: bool,
    /// Inital file path that describe the ledger to merge in `ledger_path` after starting
    pub initial_ledger_path: PathBuf,
    /// Reward for the creation of a block
    pub block_reward: Amount,
    pub operation_batch_size: usize,
    pub initial_rolls_path: PathBuf,
    pub initial_draw_seed: String,
    /// Price of a roll inside the network
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
    pub disable_block_creation: bool,
    // Max gas per block for the execution config
    pub max_gas_per_block: u64,
}

#[cfg(feature = "testing")]
#[derive(Default, Debug)]
/// In testing mode if you create a conguration with tempfiles
/// we need to keep inside this structure the `TempDir` and
/// `NamedTempFile` here since the end of the test.
/// We don't need to move or clone, only the main cfg need that and
/// the var is never used.
pub struct TempFiles {
    /// Doesn't drop tempfile on testing
    pub temp_files: Vec<tempfile::NamedTempFile>,
    /// Doesn't drop tempdir on testing
    pub temp_dir: Vec<tempfile::TempDir>,
}

impl Clone for ConsensusConfig {
    fn clone(&self) -> Self {
        Self {
            #[cfg(feature = "testing")]
            temp_files: TempFiles::default(),
            genesis_timestamp: self.genesis_timestamp,
            end_timestamp: self.end_timestamp,
            thread_count: self.thread_count,
            t0: self.t0,
            genesis_key: self.genesis_key,
            staking_keys_path: self.staking_keys_path.clone(),
            max_discarded_blocks: self.max_discarded_blocks,
            future_block_processing_max_periods: self.future_block_processing_max_periods,
            max_future_processing_blocks: self.max_future_processing_blocks,
            max_dependency_blocks: self.max_dependency_blocks,
            delta_f0: self.delta_f0,
            max_operations_per_block: self.max_operations_per_block,
            max_operations_fill_attempts: self.max_operations_fill_attempts,
            max_block_size: self.max_block_size,
            operation_validity_periods: self.operation_validity_periods,
            periods_per_cycle: self.periods_per_cycle,
            pos_lookback_cycles: self.pos_lookback_cycles,
            pos_lock_cycles: self.pos_lock_cycles,
            pos_draw_cached_cycles: self.pos_draw_cached_cycles,
            pos_miss_rate_deactivation_threshold: self.pos_miss_rate_deactivation_threshold,
            ledger_path: self.ledger_path.clone(),
            ledger_cache_capacity: self.ledger_cache_capacity,
            ledger_flush_interval: self.ledger_flush_interval,
            ledger_reset_at_startup: self.ledger_reset_at_startup,
            initial_ledger_path: self.initial_ledger_path.clone(),
            block_reward: self.block_reward,
            operation_batch_size: self.operation_batch_size,
            initial_rolls_path: self.initial_rolls_path.clone(),
            initial_draw_seed: self.initial_draw_seed.clone(),
            roll_price: self.roll_price,
            stats_timespan: self.stats_timespan,
            max_send_wait: self.max_send_wait,
            force_keep_final_periods: self.force_keep_final_periods,
            endorsement_count: self.endorsement_count,
            block_db_prune_interval: self.block_db_prune_interval,
            max_item_return_count: self.max_item_return_count,
            disable_block_creation: self.disable_block_creation,
            max_gas_per_block: self.max_gas_per_block,
        }
    }
}

impl From<&ConsensusConfig> for GraphConfig {
    fn from(cfg: &ConsensusConfig) -> Self {
        GraphConfig {
            thread_count: cfg.thread_count,
            genesis_key: cfg.genesis_key,
            max_discarded_blocks: cfg.max_discarded_blocks,
            future_block_processing_max_periods: cfg.future_block_processing_max_periods,
            max_future_processing_blocks: cfg.max_future_processing_blocks,
            max_dependency_blocks: cfg.max_dependency_blocks,
            delta_f0: cfg.delta_f0,
            operation_validity_periods: cfg.operation_validity_periods,
            periods_per_cycle: cfg.periods_per_cycle,
            initial_ledger_path: cfg.initial_ledger_path.clone(),
            block_reward: cfg.block_reward,
            roll_price: cfg.roll_price,
            force_keep_final_periods: cfg.force_keep_final_periods,
            endorsement_count: cfg.endorsement_count,
            max_item_return_count: cfg.max_item_return_count,
            ledger_path: cfg.ledger_path.clone(),
            ledger_cache_capacity: cfg.ledger_cache_capacity,
            ledger_flush_interval: cfg.ledger_flush_interval,
        }
    }
}

impl From<&ConsensusConfig> for ProofOfStakeConfig {
    fn from(cfg: &ConsensusConfig) -> Self {
        ProofOfStakeConfig {
            thread_count: cfg.thread_count,
            genesis_key: cfg.genesis_key,
            periods_per_cycle: cfg.periods_per_cycle,
            pos_lookback_cycles: cfg.pos_lookback_cycles,
            pos_lock_cycles: cfg.pos_lock_cycles,
            pos_draw_cached_cycles: cfg.pos_draw_cached_cycles,
            pos_miss_rate_deactivation_threshold: cfg.pos_miss_rate_deactivation_threshold,
            initial_rolls_path: cfg.initial_rolls_path.clone(),
            initial_draw_seed: cfg.initial_draw_seed.clone(),
            roll_price: cfg.roll_price,
            endorsement_count: cfg.endorsement_count,
        }
    }
}

impl From<&ConsensusConfig> for LedgerConfig {
    fn from(cfg: &ConsensusConfig) -> Self {
        LedgerConfig {
            thread_count: cfg.thread_count,
            ledger_path: cfg.ledger_path.clone(),
            ledger_cache_capacity: cfg.ledger_cache_capacity,
            ledger_flush_interval: cfg.ledger_flush_interval,
            initial_ledger_path: cfg.initial_ledger_path.clone(),
        }
    }
}

/// Communication async channels for the consensus worker
/// Contains consensus channels associated (protocol & execution)
/// Contains alse controller async channels (command, manager receivers and event sender)
/// Contains a sender to the pool worker commands
pub struct ConsensusWorkerChannels {
    /// Associated protocol command sender.
    pub protocol_command_sender: ProtocolCommandSender,
    /// Associated protocol event listener.
    pub protocol_event_receiver: ProtocolEventReceiver,
    /// Execution command sender.
    pub execution_controller: Box<dyn ExecutionController>,
    /// Associated Pool command sender.
    pub pool_command_sender: PoolCommandSender,
    /// Channel receiving consensus commands.
    pub controller_command_rx: mpsc::Receiver<ConsensusCommand>,
    /// Channel sending out consensus events.
    pub controller_event_tx: mpsc::Sender<ConsensusEvent>,
    /// Channel receiving consensus management commands.
    pub controller_manager_rx: mpsc::Receiver<ConsensusManagementCommand>,
}

/// Public channels associated to the consensus module.
/// Execution & Protocol Sender/Receiver
pub struct ConsensusChannels {
    pub execution_controller: Box<dyn ExecutionController>,
    pub protocol_command_sender: ProtocolCommandSender,
    pub protocol_event_receiver: ProtocolEventReceiver,
    pub pool_command_sender: PoolCommandSender,
}

impl From<&ConsensusSettings> for ConsensusConfig {
    fn from(settings: &ConsensusSettings) -> Self {
        #[cfg(feature = "testing")]
        /// If the feature `testing` is activated we force the unit
        /// test values to be used for a default initialisation.
        use massa_models::constants::default_testing::*;
        #[cfg(not(feature = "testing"))]
        use massa_models::constants::*;
        ConsensusConfig {
            #[cfg(feature = "testing")]
            temp_files: TempFiles::default(), // No need to clone
            genesis_timestamp: *GENESIS_TIMESTAMP,
            end_timestamp: *END_TIMESTAMP,
            thread_count: THREAD_COUNT,
            t0: T0,
            genesis_key: *GENESIS_KEY,
            staking_keys_path: settings.staking_keys_path.clone(),
            max_discarded_blocks: settings.max_discarded_blocks,
            future_block_processing_max_periods: settings.future_block_processing_max_periods,
            max_future_processing_blocks: settings.max_future_processing_blocks,
            max_dependency_blocks: settings.max_dependency_blocks,
            delta_f0: DELTA_F0,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            max_operations_fill_attempts: settings.max_operations_fill_attempts,
            max_block_size: MAX_BLOCK_SIZE,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            pos_lookback_cycles: POS_LOOKBACK_CYCLES,
            pos_lock_cycles: POS_LOCK_CYCLES,
            pos_draw_cached_cycles: settings.pos_draw_cached_cycles,
            pos_miss_rate_deactivation_threshold: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
            ledger_path: settings.ledger_path.clone(),
            ledger_cache_capacity: settings.ledger_cache_capacity,
            ledger_flush_interval: settings.ledger_flush_interval,
            ledger_reset_at_startup: settings.ledger_reset_at_startup,
            initial_ledger_path: settings.initial_ledger_path.clone(),
            block_reward: BLOCK_REWARD,
            operation_batch_size: settings.operation_batch_size,
            initial_rolls_path: settings.initial_rolls_path.clone(),
            initial_draw_seed: INITIAL_DRAW_SEED.to_string(),
            roll_price: ROLL_PRICE,
            stats_timespan: settings.stats_timespan,
            max_send_wait: settings.max_send_wait,
            force_keep_final_periods: settings.force_keep_final_periods,
            endorsement_count: ENDORSEMENT_COUNT,
            block_db_prune_interval: settings.block_db_prune_interval,
            max_item_return_count: settings.max_item_return_count,
            disable_block_creation: settings.disable_block_creation,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
        }
    }
}

impl From<ConsensusSettings> for ConsensusConfig {
    fn from(settings: ConsensusSettings) -> Self {
        #[cfg(feature = "testing")]
        /// If the feature `testing` is activated we force the unit
        /// test values to be used for a default initialisation.
        use massa_models::constants::default_testing::*;
        #[cfg(not(feature = "testing"))]
        use massa_models::constants::*;
        ConsensusConfig {
            #[cfg(feature = "testing")]
            temp_files: Default::default(),
            genesis_timestamp: *GENESIS_TIMESTAMP,
            end_timestamp: *END_TIMESTAMP,
            thread_count: THREAD_COUNT,
            t0: T0,
            genesis_key: *GENESIS_KEY,
            staking_keys_path: settings.staking_keys_path,
            max_discarded_blocks: settings.max_discarded_blocks,
            future_block_processing_max_periods: settings.future_block_processing_max_periods,
            max_future_processing_blocks: settings.max_future_processing_blocks,
            max_dependency_blocks: settings.max_dependency_blocks,
            delta_f0: DELTA_F0,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            max_operations_fill_attempts: settings.max_operations_fill_attempts,
            max_block_size: MAX_BLOCK_SIZE,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            pos_lookback_cycles: POS_LOOKBACK_CYCLES,
            pos_lock_cycles: POS_LOCK_CYCLES,
            pos_draw_cached_cycles: settings.pos_draw_cached_cycles,
            pos_miss_rate_deactivation_threshold: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
            ledger_path: settings.ledger_path,
            ledger_cache_capacity: settings.ledger_cache_capacity,
            ledger_flush_interval: settings.ledger_flush_interval,
            ledger_reset_at_startup: settings.ledger_reset_at_startup,
            initial_ledger_path: settings.initial_ledger_path,
            block_reward: BLOCK_REWARD,
            operation_batch_size: settings.operation_batch_size,
            initial_rolls_path: settings.initial_rolls_path,
            initial_draw_seed: INITIAL_DRAW_SEED.to_string(),
            roll_price: ROLL_PRICE,
            stats_timespan: settings.stats_timespan,
            max_send_wait: settings.max_send_wait,
            force_keep_final_periods: settings.force_keep_final_periods,
            endorsement_count: ENDORSEMENT_COUNT,
            block_db_prune_interval: settings.block_db_prune_interval,
            max_item_return_count: settings.max_item_return_count,
            disable_block_creation: settings.disable_block_creation,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
        }
    }
}

/// Defining the default consensus configuration for both
/// testing and not testing.
///
/// Use the feature testing to load mocked default variable
/// This is default variable but the configuration is a dynamic
/// configuration passed as dependences with no particular lifetime
#[cfg(feature = "testing")]
impl Default for ConsensusSettings {
    fn default() -> Self {
        /// If the feature `testing` is activated we force the unit
        /// test values to be used for a default initialisation.
        use massa_models::constants::default_testing::*;
        Self {
            staking_keys_path: Default::default(),
            max_discarded_blocks: MAX_DISCARED_BLOCKS,
            future_block_processing_max_periods: FUTURE_BLOCK_PROCESSING_MAX_PERIODS,
            max_future_processing_blocks: MAX_FUTURE_PROCESSING_BLOCK,
            max_dependency_blocks: MAX_DEPENDENCY_BLOCK,
            max_operations_fill_attempts: MAX_OPERATION_FILL_ATTEMPTS,
            pos_draw_cached_cycles: POS_DRAW_CACHED_CYCLE,
            ledger_path: Default::default(),
            ledger_cache_capacity: LEDGER_CACHE_CAPACITY,
            ledger_flush_interval: *LEDGER_FLUSH_INTERVAL,
            ledger_reset_at_startup: LEDGER_RESET_AT_STARTUP,
            initial_ledger_path: Default::default(),
            operation_batch_size: OPERATION_BATCH_SIZE,
            initial_rolls_path: Default::default(),
            stats_timespan: *STATS_TIMESPAN,
            max_send_wait: *MAX_SEND_WAIT,
            force_keep_final_periods: FORCE_KEEP_FINAL_PERIOD,
            block_db_prune_interval: *BLOCK_DB_PRUNE_INTERVAL,
            max_item_return_count: MAX_ITEM_RETURN_COUNT,
            disable_block_creation: DISABLE_BLOCK_CREATION,
        }
    }
}

#[cfg(feature = "testing")]
impl From<&std::path::Path> for ConsensusConfig {
    /// Utility trait used in tests to get a full consensus configuration from an initial ledger path
    fn from(initial_ledger_path: &std::path::Path) -> Self {
        let mut staking_keys = Vec::new();
        for _ in 0..2 {
            staking_keys.push(massa_signature::generate_random_private_key());
        }
        massa_models::init_serialization_context(massa_models::SerializationContext::default());
        ConsensusSettings {
            staking_keys_path: crate::tools::generate_staking_keys_file(&staking_keys)
                .path()
                .to_path_buf(),
            ledger_path: tempfile::tempdir()
                .expect("cannot create temp dir")
                .path()
                .to_path_buf(),
            initial_ledger_path: initial_ledger_path.to_path_buf(),
            initial_rolls_path: tempfile::tempdir()
                .expect("cannot create temp dir")
                .path()
                .to_path_buf(),
            ..Default::default()
        }
        .into()
    }
}

#[cfg(feature = "testing")]
/**
 * Create the default value of `ConsensusConfig`.
 *
 * Configuration has default values described in crate `massa-models`.
 * The most of `ConsensusConfig` values have in test mode a default value.
 *
 * You can create a ConsensusConfig with classic default values and redefining
 * dynamically the values of desired parameters:
 *
 * ```ignore
 * let cfg = ConsensusConfig {
 *     max_discarded_blocks: 25,
 *     ..Default::default()
 * };
 * ```
 *
 * You can also look at the divers `default()` implementation bellow. For example that
 * one is used to initialise the _default paths_ :
 *
 * ```ignore
 * let cfg = ConensusConfig {
 *     max_discarded_blocks: 21,
 *     ..ConsensusConfig::default_with_paths(),
 * };
 * ```
 */
impl Default for ConsensusConfig {
    fn default() -> Self {
        use massa_models::constants::default_testing::*;
        massa_models::init_serialization_context(massa_models::SerializationContext::default());
        let tempdir = tempfile::tempdir().expect("cannot create temp dir for the ledger path");
        let path_buf = tempdir.path().to_path_buf();
        Self {
            temp_files: TempFiles {
                temp_dir: vec![tempdir],
                temp_files: vec![],
            },
            // reset genesis timestamp because we are in test mode that can take a while to process
            genesis_timestamp: MassaTime::now().expect("Impossible to reset the timestamp in test"),
            end_timestamp: *END_TIMESTAMP,
            thread_count: THREAD_COUNT,
            t0: T0,
            genesis_key: *GENESIS_KEY,
            staking_keys_path: Default::default(),
            max_discarded_blocks: MAX_DISCARED_BLOCKS,
            future_block_processing_max_periods: FUTURE_BLOCK_PROCESSING_MAX_PERIODS,
            max_future_processing_blocks: MAX_FUTURE_PROCESSING_BLOCK,
            max_dependency_blocks: MAX_DEPENDENCY_BLOCK,
            delta_f0: DELTA_F0,
            max_operations_per_block: MAX_OPERATIONS_PER_BLOCK,
            max_operations_fill_attempts: MAX_OPERATION_FILL_ATTEMPTS,
            max_block_size: MAX_BLOCK_SIZE,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            pos_lookback_cycles: POS_LOOKBACK_CYCLES,
            pos_lock_cycles: POS_LOCK_CYCLES,
            pos_draw_cached_cycles: POS_DRAW_CACHED_CYCLE,
            pos_miss_rate_deactivation_threshold: *POS_MISS_RATE_DEACTIVATION_THRESHOLD,
            ledger_path: path_buf,
            ledger_cache_capacity: LEDGER_CACHE_CAPACITY,
            ledger_flush_interval: *LEDGER_FLUSH_INTERVAL,
            ledger_reset_at_startup: LEDGER_RESET_AT_STARTUP,
            initial_ledger_path: Default::default(),
            block_reward: BLOCK_REWARD,
            operation_batch_size: OPERATION_BATCH_SIZE,
            initial_rolls_path: Default::default(),
            initial_draw_seed: INITIAL_DRAW_SEED.to_string(),
            roll_price: ROLL_PRICE,
            stats_timespan: *STATS_TIMESPAN,
            max_send_wait: *MAX_SEND_WAIT,
            force_keep_final_periods: FORCE_KEEP_FINAL_PERIOD,
            endorsement_count: ENDORSEMENT_COUNT,
            block_db_prune_interval: *BLOCK_DB_PRUNE_INTERVAL,
            max_item_return_count: MAX_ITEM_RETURN_COUNT,
            disable_block_creation: DISABLE_BLOCK_CREATION,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
        }
    }
}

#[cfg(feature = "testing")]
/**
 * The following implementation correspond to tools used in unit tests
 * It allow you to get a default `ConsensusConfig` (that isn't possible without
 * the feature *testing*) with already setted/default `initial_ledger_path`,
 * `staking_keys_path` and `initial_rolls_path`.
 *
 * Used to radically reduce code duplication in unit tests of Consensus.
 */
impl ConsensusConfig {
    pub fn default_with_paths() -> Self {
        use crate::tools::*;
        let staking_keys: Vec<PrivateKey> = (0..1)
            .map(|_| massa_signature::generate_random_private_key())
            .collect();
        let ledger_file = generate_ledger_file(&std::collections::HashMap::new());
        let staking_file = generate_staking_keys_file(&staking_keys);
        let rolls_file = generate_default_roll_counts_file(staking_keys);
        ConsensusConfig {
            initial_ledger_path: ledger_file.path().to_path_buf(),
            staking_keys_path: staking_file.path().to_path_buf(),
            initial_rolls_path: rolls_file.path().to_path_buf(),
            temp_files: TempFiles {
                temp_files: vec![ledger_file, staking_file, rolls_file],
                temp_dir: vec![],
            },
            ..Default::default()
        }
    }
    pub fn default_with_staking_keys(staking_keys: &[PrivateKey]) -> Self {
        use crate::tools::*;
        let ledger_file = generate_ledger_file(&std::collections::HashMap::new());
        let staking_file = generate_staking_keys_file(staking_keys);
        let rolls_file = generate_default_roll_counts_file(staking_keys.to_vec());
        ConsensusConfig {
            initial_ledger_path: ledger_file.path().to_path_buf(),
            staking_keys_path: staking_file.path().to_path_buf(),
            initial_rolls_path: rolls_file.path().to_path_buf(),
            temp_files: TempFiles {
                temp_files: vec![ledger_file, staking_file, rolls_file],
                temp_dir: vec![],
            },
            ..Default::default()
        }
    }
    pub fn default_with_staking_keys_and_ledger(
        staking_keys: &[PrivateKey],
        ledger: &std::collections::HashMap<
            massa_models::Address,
            massa_models::ledger_models::LedgerData,
        >,
    ) -> Self {
        use crate::tools::*;
        let ledger_file = generate_ledger_file(ledger);
        let staking_file = generate_staking_keys_file(staking_keys);
        let rolls_file = generate_default_roll_counts_file(staking_keys.to_vec());
        ConsensusConfig {
            initial_ledger_path: ledger_file.path().to_path_buf(),
            staking_keys_path: staking_file.path().to_path_buf(),
            initial_rolls_path: rolls_file.path().to_path_buf(),
            temp_files: TempFiles {
                temp_files: vec![ledger_file, staking_file, rolls_file],
                temp_dir: vec![],
            },
            ..Default::default()
        }
    }
}
