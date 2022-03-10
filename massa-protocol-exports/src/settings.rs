// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_time::MassaTime;
use serde::Deserialize;

/// Protocol Configuration
#[derive(Debug, Deserialize, Clone, Copy)]
pub struct ProtocolSettings {
    pub ask_block_timeout: MassaTime,
    pub operation_knowledge_view_config: KnowledgeViewConfig,
    pub endorsement_knowledge_view_config: KnowledgeViewConfig,
    pub block_knowledge_view_config: KnowledgeViewConfig,
    pub max_simultaneous_ask_blocks_per_node: usize,
    /// Max wait time for sending a Network or Node event.
    pub max_send_wait: MassaTime,
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub struct KnowledgeViewConfig {
    max_known: usize,
    max_wanted: usize,
    max_asked: usize,
}
