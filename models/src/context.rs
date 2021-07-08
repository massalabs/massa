/// a context for model serialization/deserialization
#[derive(Debug, Clone)]
pub struct SerializationContext {
    pub max_block_operations: u32,
    pub parent_count: u8,
    pub max_block_size: u32,
    pub max_peer_list_length: u32,
    pub max_message_size: u32,
}
