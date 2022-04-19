// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::Deserialize;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ProtocolSettings {
    /// after `ask_block_timeout` milliseconds we try to ask a block to another node
    pub ask_block_timeout: MassaTime,
    /// max known blocks per node kept in memory
    pub max_node_known_blocks_size: usize,
    /// max wanted blocks per node kept in memory
    pub max_node_wanted_blocks_size: usize,
    /// max known operations per node kept in memory
    pub max_known_ops_size: usize,
    /// max known endorsements per node kept in memory
    pub max_known_endorsements_size: usize,
    /// we ask for the same block `max_simultaneous_ask_blocks_per_node` times at the same time
    pub max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: MassaTime,
    /// Maximum number of batches in the memory buffer.
    /// Dismiss the new batches if overflow
    pub operation_batch_buffer_capacity: usize,
    /// Start processing batches in the buffer each `operation_batch_proc_period` in millisecond
    pub operation_batch_proc_period: MassaTime,
    /// All operations asked are prune each `operation_asked_pruning_period` millisecond
    pub asked_operations_pruning_period: MassaTime,
    /// Maximum of operations sent in one message.
    pub max_operations_per_message: u64,
}
