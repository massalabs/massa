// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::Deserialize;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ProtocolSettings {
    /// after ask_block_timeout millis we try to ask a block to another node
    pub ask_block_timeout: MassaTime,
    /// max known blocks per node kept in memory
    pub max_node_known_blocks_size: usize,
    /// max wanted blocks per node kept in memory
    pub max_node_wanted_blocks_size: usize,
    /// max known operations per node kept in memory
    pub max_known_ops_size: usize,
    /// max known endorsements per node kept in memory
    pub max_known_endorsements_size: usize,
    /// we ask for the same block max_simultaneous_ask_blocks_per_node times at the same time
    pub max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: MassaTime,
}
