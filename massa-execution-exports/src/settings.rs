// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module provides the structures used to provide configuration parameters to the Execution system

use massa_models::amount::Amount;
use massa_time::MassaTime;
use num::rational::Ratio;

/// Execution module configuration
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// read-only execution request queue length
    pub readonly_queue_length: usize,
    /// maximum number of SC output events kept in cache
    pub max_final_events: usize,
    /// maximum available gas for asynchronous messages execution
    pub max_async_gas: u64,
    /// maximum gas per block
    pub max_gas_per_block: u64,
    /// number of threads
    pub thread_count: u8,
    /// price of a roll inside the network
    pub roll_price: Amount,
    /// extra lag to add on the execution cursor to improve performance
    pub cursor_delay: MassaTime,
    /// time compensation in milliseconds
    pub clock_compensation: i64,
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
}
