use super::mock_network_controller::MockNetworkController;
use crate::common::NodeId;
use crate::network::NetworkCommand;
use crate::protocol::{ProtocolConfig, ProtocolEvent, ProtocolEventReceiver};
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey, SignatureEngine},
};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use time::UTime;
use tokio::time::sleep;

#[derive(Debug, Clone)]
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
        network_controller.new_connection(info.id).await;
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
        ProtocolConfig {
            ask_block_timeout: 500.into(),
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

pub async fn wait_protocol_event<F>(
    protocol_event_receiver: &mut ProtocolEventReceiver,
    timeout: UTime,
    filter_map: F,
) -> Option<ProtocolEvent>
where
    F: Fn(ProtocolEvent) -> Option<ProtocolEvent>,
{
    let timer = sleep(timeout.into());
    tokio::pin!(timer);
    loop {
        tokio::select! {
            evt_opt = protocol_event_receiver.wait_event() => match evt_opt {
                Ok(orig_evt) => if let Some(res_evt) = filter_map(orig_evt) { return Some(res_evt); },
                _ => return None
            },
            _ = &mut timer => return None
        }
    }
}

pub async fn assert_hash_asked_to_node(
    hash_1: Hash,
    node_id: NodeId,
    network_controller: &mut MockNetworkController,
) {
    let ask_for_block_cmd_filter = |cmd| match cmd {
        cmd @ NetworkCommand::AskForBlock { .. } => Some(cmd),
        b => {
            println!("{:?}", b);
            None
        }
    };
    let (ask_to_node_id, asked_for_hash) = match network_controller
        .wait_command(1000.into(), ask_for_block_cmd_filter)
        .await
    {
        Some(NetworkCommand::AskForBlock { node, hash }) => (node, hash),
        cmd => panic!("Unexpected network command. {:?}", cmd),
    };
    assert_eq!(hash_1, asked_for_hash);
    assert_eq!(ask_to_node_id, node_id);
}
