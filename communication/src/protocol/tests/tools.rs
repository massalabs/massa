use crate::protocol::{NodeId, ProtocolConfig};
use crypto::signature::{PrivateKey, SignatureEngine};
use models::SerializationContext;
use time::UTime;

// generate random node ID (public key) and private key
pub fn generate_node_keys() -> (PrivateKey, NodeId) {
    let signature_engine = SignatureEngine::new();
    let private_key = SignatureEngine::generate_random_private_key();
    let self_node_id = NodeId(signature_engine.derive_public_key(&private_key));
    (private_key, self_node_id)
}

// create a ProtocolConfig with typical values
pub fn create_protocol_config() -> (ProtocolConfig, SerializationContext) {
    (
        ProtocolConfig {
            message_timeout: UTime::from(5000u64),
            ask_peer_list_interval: UTime::from(50000u64),
        },
        SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
        },
    )
}
