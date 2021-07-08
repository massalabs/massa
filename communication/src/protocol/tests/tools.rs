use super::mock_network_controller::MockNetworkController;
use crate::common::NodeId;
use crate::protocol::ProtocolConfig;
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey, SignatureEngine},
};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};

pub struct NodeInfo {
    pub private_key: PrivateKey,
    pub id: NodeId,
}

pub fn create_node(signature_engine: &SignatureEngine) -> NodeInfo {
    let private_key = SignatureEngine::generate_random_private_key();
    let id = NodeId(signature_engine.derive_public_key(&private_key));
    NodeInfo { private_key, id }
}

pub async fn create_and_connect_nodes(
    num: usize,
    signature_engine: &SignatureEngine,
    network_controller: &mut MockNetworkController,
) -> Vec<NodeInfo> {
    let mut nodes = vec![];
    for _ in 0..num {
        // Create a node, and connect it with protocol.
        let info = create_node(&signature_engine);
        network_controller.new_connection(&info.id).await;
        nodes.push(info);
    }
    nodes
}

/// Creates a block for use in protocol,
/// without paying attention to consensus related things
/// like slot, parents, and merkle root.
pub fn create_block(
    private_key: &PrivateKey,
    public_key: &PublicKey,
    serialization_context: &SerializationContext,
    signature_engine: &mut SignatureEngine,
) -> Block {
    let example_hash = Hash::hash("default_val".as_bytes());

    let (_, header) = BlockHeader::new_signed(
        signature_engine,
        private_key,
        BlockHeaderContent {
            creator: public_key.clone(),
            slot: Slot::new(0, 0),
            parents: Vec::new(),
            out_ledger_hash: example_hash,
            operation_merkle_root: example_hash,
        },
        &serialization_context,
    )
    .unwrap();

    Block {
        header,
        operations: Vec::new(),
    }
}

// create a ProtocolConfig with typical values
pub fn create_protocol_config() -> (ProtocolConfig, SerializationContext) {
    (
        ProtocolConfig {},
        SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
        },
    )
}
