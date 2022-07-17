// Copyright (c) 2022 MASSA LABS <info@massa.net>

#![allow(clippy::assertions_on_constants)]

use massa_models::Amount;
use massa_signature::PrivateKey;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, usize};

/// Graph configuration
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GraphConfig {
    /// Number of threads
    pub thread_count: u8,
    /// Private key to sign genesis blocks.
    pub genesis_key: PrivateKey,
    /// Maximum number of blocks allowed in discarded blocks.
    pub max_discarded_blocks: usize,
    /// If a block `is future_block_processing_max_periods` periods in the future, it is just discarded.
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
    /// Initial file path that describe the ledger to merge in `ledger_path` after starting
    pub initial_ledger_path: PathBuf,
    /// Reward for the creation of a block
    pub block_reward: Amount,
    /// Price of a roll inside the network
    pub roll_price: Amount,
    /// force keep at least this number of final periods in RAM for each thread
    pub force_keep_final_periods: u64,
    /// target number of endorsement per block
    pub endorsement_count: u32,
    /// pub `block_db_prune_interval`: `MassaTime`,
    pub max_item_return_count: usize,
}
