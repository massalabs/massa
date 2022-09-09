// Copyright (c) 2022 MASSA LABS <info@massa.net>
#![allow(clippy::assertions_on_constants)]
//! Definition & Implementation of the consensus settings
//! -----------------------------------------------------
//!
//! # Configurations
//!
//! * `setting`: read from user settings file
//! * `config`: merge of settings and hard-coded configuration that shouldn't be
//!           modified by user.
//!
//! This file is allowed to use a lot of constants from `massa-models` as all
//! other files named `settings.rs` or `config.rs`.
//!
//! The `ConsensusSettings` is the most basic and complete configuration in the
//! node. You can get almost every configuration from that one.
//!
//! `From<ConsensusSettings> impl *`:
//! - `ConsensusConfig`: Create a configuration merging user settings and hard-coded values
//!                      (see `/massa-models/node_configuration/*`)
//!
//! `From<&ConsensusConfig> impl *`:
//! - `GraphConfig`
//! - `LedgerConfig`
//! - `ProofOfStakeConfig`
//!
//! > Development note: We clone the values on getting a configuration from another.
//!
//! # Usage of constants
//!
//! The default configuration is loaded from the `massa-models` crate. You shouldn't
//! write an hard-coded value in the following file but create a new value in
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
use massa_graph::settings::GraphConfig;
use massa_pool_exports::PoolController;
use massa_pos_exports::SelectorController;
use massa_protocol_exports::{ProtocolCommandSender, ProtocolEventReceiver};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use tokio::sync::mpsc;

use crate::{
    commands::{ConsensusCommand, ConsensusManagementCommand},
    events::ConsensusEvent,
};

/// Consensus full configuration (static + user defined)
#[derive(Debug, Clone)]
pub struct ConsensusConfig {
    /// Time in milliseconds when the blockclique started.
    pub genesis_timestamp: MassaTime,
    /// TESTNET: time when the blockclique is ended.
    pub end_timestamp: Option<MassaTime>,
    /// Number of threads
    pub thread_count: u8,
    /// Time between the periods in the same thread.
    pub t0: MassaTime,
    /// `KeyPair` to sign genesis blocks.
    pub genesis_key: KeyPair,
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// If a block is `future_block_processing_max_periods` periods in the future, it is just discarded.
    pub future_block_processing_max_periods: u64,
    /// Maximum number of blocks allowed in `FutureIncomingBlocks`.
    pub max_future_processing_blocks: usize,
    /// Maximum number of blocks allowed in `DependencyWaitingBlocks`.
    pub max_dependency_blocks: usize,
    /// Threshold for fitness.
    pub delta_f0: u64,
    /// Maximum operation validity period count
    pub operation_validity_periods: u64,
    /// cycle duration in periods
    pub periods_per_cycle: u64,
    /// stats time span
    pub stats_timespan: MassaTime,
    /// max event send wait
    pub max_send_wait: MassaTime,
    /// force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    /// target number of endorsement per block
    pub endorsement_count: u32,
    /// old blocks are pruned every `block_db_prune_interval`
    pub block_db_prune_interval: MassaTime,
    /// max number of items returned while querying
    pub max_item_return_count: usize,
    /// Max gas per block for the execution configuration
    pub max_gas_per_block: u64,
    /// channel size
    pub channel_size: usize,
}

impl From<&ConsensusConfig> for GraphConfig {
    fn from(cfg: &ConsensusConfig) -> Self {
        GraphConfig {
            thread_count: cfg.thread_count,
            genesis_key: cfg.genesis_key.clone(),
            max_discarded_blocks: cfg.max_discarded_blocks,
            future_block_processing_max_periods: cfg.future_block_processing_max_periods,
            max_future_processing_blocks: cfg.max_future_processing_blocks,
            max_dependency_blocks: cfg.max_dependency_blocks,
            delta_f0: cfg.delta_f0,
            operation_validity_periods: cfg.operation_validity_periods,
            periods_per_cycle: cfg.periods_per_cycle,
            force_keep_final_periods: cfg.force_keep_final_periods,
            endorsement_count: cfg.endorsement_count,
            max_item_return_count: cfg.max_item_return_count,
        }
    }
}

/// Communication asynchronous channels for the consensus worker
/// Contains consensus channels associated (protocol & execution)
/// Contains also controller asynchronous channels (command, manager receivers and event sender)
/// Contains a sender to the pool worker commands
pub struct ConsensusWorkerChannels {
    /// Associated protocol command sender.
    pub protocol_command_sender: ProtocolCommandSender,
    /// Associated protocol event listener.
    pub protocol_event_receiver: ProtocolEventReceiver,
    /// Execution command sender.
    pub execution_controller: Box<dyn ExecutionController>,
    /// Associated Pool command sender.
    pub pool_command_sender: Box<dyn PoolController>,
    /// Selector controller
    pub selector_controller: Box<dyn SelectorController>,
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
    /// outgoing link to execution component
    pub execution_controller: Box<dyn ExecutionController>,
    /// outgoing link to protocol component
    pub protocol_command_sender: ProtocolCommandSender,
    /// incoming link to protocol component
    pub protocol_event_receiver: ProtocolEventReceiver,
    /// outgoing link to pool component
    pub pool_command_sender: Box<dyn PoolController>,
    /// selector controller
    pub selector_controller: Box<dyn SelectorController>,
}

#[cfg(feature = "testing")]
///
/// Create the default value of `ConsensusConfig`.
///
/// Configuration has default values described in crate `massa-models`.
/// The most of `ConsensusConfig` values have in test mode a default value.
///
/// You can create a `ConsensusConfig` with classic default values and redefining
/// dynamically the values of desired parameters:
///
/// ```ignore
/// let cfg = ConsensusConfig {
///     max_discarded_blocks: 25,
///     ..Default::default()
/// };
/// ```
///
/// You can also look at the divers `default()` implementation bellow. For example that
/// one is used to initialize the _default paths_ :
///
/// ```ignore
/// let cfg = ConsensusConfig {
///     max_discarded_blocks: 21,
///     ..ConsensusConfig::default_with_paths(),
/// };
/// ```
///
impl Default for ConsensusConfig {
    fn default() -> Self {
        use massa_models::config::*;
        Self {
            // reset genesis timestamp because we are in test mode that can take a while to process
            genesis_timestamp: MassaTime::now(0)
                .expect("Impossible to reset the timestamp in test"),
            end_timestamp: *END_TIMESTAMP,
            thread_count: THREAD_COUNT,
            t0: T0,
            genesis_key: GENESIS_KEY.clone(),
            max_discarded_blocks: 100,
            future_block_processing_max_periods: 2,
            max_future_processing_blocks: 10,
            max_dependency_blocks: 100,
            delta_f0: DELTA_F0,
            operation_validity_periods: OPERATION_VALIDITY_PERIODS,
            periods_per_cycle: PERIODS_PER_CYCLE,
            stats_timespan: MassaTime::from_millis(1000),
            max_send_wait: MassaTime::from_millis(1000),
            force_keep_final_periods: 20,
            endorsement_count: ENDORSEMENT_COUNT,
            block_db_prune_interval: MassaTime::from_millis(1000),
            max_item_return_count: 100,
            max_gas_per_block: MAX_GAS_PER_BLOCK,
            channel_size: CHANNEL_SIZE,
        }
    }
}
