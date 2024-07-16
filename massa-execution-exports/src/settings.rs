// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module provides the structures used to provide configuration parameters to the Execution system

use massa_models::amount::Amount;
use massa_sc_runtime::GasCosts;
use massa_time::MassaTime;
use num::rational::Ratio;
use std::path::PathBuf;

/// Storage cost constants
#[derive(Debug, Clone, Copy)]
pub struct StorageCostsConstants {
    /// Cost per byte in ledger
    pub ledger_cost_per_byte: Amount,
    /// Ledger entry base cost
    pub ledger_entry_base_cost: Amount,
    /// Ledger entry datastore base cost
    pub ledger_entry_datastore_base_cost: Amount,
}

/// Execution module configuration
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// read-only execution request queue length
    pub readonly_queue_length: usize,
    /// maximum number of SC output events kept in cache
    pub max_final_events: usize,
    /// maximum available gas for asynchronous messages execution
    pub max_async_gas: u64,
    /// constant cost for async messages
    pub async_msg_cst_gas_cost: u64,
    /// maximum gas per block
    pub max_gas_per_block: u64,
    /// number of threads
    pub thread_count: u8,
    /// price of a roll inside the network
    pub roll_price: Amount,
    /// extra lag to add on the execution cursor to improve performance
    pub cursor_delay: MassaTime,
    /// genesis timestamp
    pub genesis_timestamp: MassaTime,
    /// period duration
    pub t0: MassaTime,
    /// block creation reward
    pub block_reward: Amount,
    /// operation validity period
    pub operation_validity_period: u64,
    /// endorsement count
    pub endorsement_count: u64,
    /// periods per cycle
    pub periods_per_cycle: u64,
    /// duration of the statistics time window
    pub stats_time_window_duration: MassaTime,
    /// Max miss ratio for auto roll sell
    pub max_miss_ratio: Ratio<u64>,
    /// Max function length in call sc
    pub max_function_length: u16,
    /// Max parameter length in call sc
    pub max_parameter_length: u32,
    /// Max size of a datastore key
    pub max_datastore_key_length: u8,
    /// Max bytecode size
    pub max_bytecode_size: u64,
    /// Max datastore value size
    pub max_datastore_value_size: u64,
    /// Storage cost constants
    pub storage_costs_constants: StorageCostsConstants,
    /// Max gas for read only executions
    pub max_read_only_gas: u64,
    /// Gas costs
    pub gas_costs: GasCosts,
    /// Gas used by a transaction, a roll buy or a roll sell)
    pub base_operation_gas_cost: u64,
    /// last start period, used to attach to the correct execution slot if the network has restarted
    pub last_start_period: u64,
    /// Path to the hard drive cache storage
    pub hd_cache_path: PathBuf,
    /// Maximum number of entries we want to keep in the LRU cache
    pub lru_cache_size: u32,
    /// Maximum number of entries we want to keep in the HD cache
    pub hd_cache_size: usize,
    /// Amount of entries removed when `hd_cache_size` is reached
    pub snip_amount: usize,
    /// Number of roll to remove per denunciation
    pub roll_count_to_slash_on_denunciation: u64,
    /// Denunciation expire delta
    pub denunciation_expire_periods: u64,
    /// whether slot execution outputs broadcast is enabled
    pub broadcast_enabled: bool,
    /// slot execution outputs channel capacity
    pub broadcast_slot_execution_output_channel_capacity: usize,
    /// max size of event data, in bytes
    pub max_event_size: usize,
    /// chain id
    pub chain_id: u64,
    /// whether slot execution traces broadcast is enabled
    pub broadcast_traces_enabled: bool,
    /// slot execution traces channel capacity
    pub broadcast_slot_execution_traces_channel_capacity: usize,
    /// Max execution traces slot to keep in trace history cache
    pub max_execution_traces_slot_limit: usize,
    /// Where to dump blocks
    pub block_dump_folder_path: PathBuf,
    /// Max deferred call future slot
    pub max_deferred_call_future_slots: u64,
}
