// Copyright (c) 2022 MASSA LABS <info@massa.net>

//! This module provides the structures used to provide configuration parameters to the Execution system

use massa_time::MassaTime;

/// Execution module configuration
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// read-only execution request queue length
    pub readonly_queue_length: usize,
    /// maximum number of SC output events kept in cache
    pub max_final_events: usize,
    /// maximum available gas for asynchronous messages execution
    pub max_async_gas: u64,
    /// number of threads
    pub thread_count: u8,
    /// extra lag to add on the execution cursor to improve performance
    pub cursor_delay: MassaTime,
    /// time compensation in milliseconds
    pub clock_compensation: i64,
    /// genesis timestamp
    pub genesis_timestamp: MassaTime,
    /// period duration
    pub t0: MassaTime,
}
