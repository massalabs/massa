// Copyright (c) 2021 MASSA LABS <info@massa.net>

use serde::Deserialize;
use time::UTime;

pub const CHANNEL_SIZE: usize = 256;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ProtocolSettings {
    pub ask_block_timeout: UTime,
    pub max_node_known_blocks_size: usize,
    pub max_node_wanted_blocks_size: usize,
    pub max_known_ops_size: usize,
    pub max_known_endorsements_size: usize,
    pub max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: UTime,
}
