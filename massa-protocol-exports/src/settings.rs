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
    pub operation_batch_buffer_capacity: usize,
    /// Start processing batches in the buffer each `operation_batch_proc_period` in millisecond
    pub operation_batch_proc_period: u64,
    /// All operations asked are prune each `operation_asked_pruning_period` millisecond
    /// todo: link algorithm documentation
    pub operation_asked_pruning_period: u64,
}
