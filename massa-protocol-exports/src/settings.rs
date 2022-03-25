// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::Deserialize;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ProtocolSettings {
    pub ask_block_timeout: MassaTime,
    pub max_node_known_blocks_size: usize,
    pub max_node_wanted_blocks_size: usize,
    pub max_known_ops_size: usize,
    pub max_known_endorsements_size: usize,
    pub max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: MassaTime,
    /// Maximum number of batches in the memory buffer.
    /// Dismiss the new batches if overflow
    pub operation_batch_buffer_capacity: usize,
    /// Start processing batches in the buffer each `operation_batch_proc_period` in millisecond
    pub operation_batch_proc_period: MassaTime,
    /// All operations asked are prune each `operation_asked_pruning_period` millisecond
    pub asked_operations_pruning_period: u64,
    /// All operations asked are prune each `operation_asked_pruning_period` millisecond
    pub max_operations_per_message: u64,
    /// Pruning asked op timer
    pub asked_ops_lifetime: MassaTime,
}

impl ProtocolSettings {
    pub fn get_batch_send_period(&self) -> Duration {
        if self.max_op_batch_per_sec_per_node == 0 {
            // panic or what
            Duration::from_secs(1)
        } else {
            Duration::from_millis(1000 / self.max_op_batch_per_sec_per_node as u64)
        }
    }
}
