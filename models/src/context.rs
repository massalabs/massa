use serde::{Deserialize, Serialize};

/// a context for model serialization/deserialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializationContext {
    pub max_block_operations: u32,
    pub parent_count: u8,
    pub max_block_size: u32,
    pub max_peer_list_length: u32,
    pub max_message_size: u32,
    pub max_bootstrap_blocks: u32,
    pub max_bootstrap_cliques: u32,
    pub max_bootstrap_deps: u32,
    pub max_bootstrap_children: u32,
    pub max_bootstrap_message_size: u32,
    pub max_ask_blocks_per_message: u32,
    pub max_operations_per_message: u32,
}
