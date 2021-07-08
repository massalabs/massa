use super::mock_network_controller::MockNetworkController;
use crate::common::NodeId;
use crate::network::NetworkCommand;
use crate::protocol::{ProtocolConfig, ProtocolEvent, ProtocolEventReceiver};
use crypto::{
    hash::Hash,
    signature::{PrivateKey, PublicKey, SignatureEngine},
};
use models::{Block, BlockHeader, BlockHeaderContent, SerializationContext, Slot};
use std::collections::HashMap;
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
            max_node_known_blocks_size: 100,
            max_node_wanted_blocks_size: 100,
            max_simultaneous_ask_blocks_per_node: 10,
        },
        SerializationContext {
            max_block_size: 1024 * 1024,
            max_block_operations: 1024,
            parent_count: 2,
            max_peer_list_length: 128,
            max_message_size: 3 * 1024 * 1024,
            max_bootstrap_blocks: 100,
            max_bootstrap_cliques: 100,
            max_bootstrap_deps: 100,
            max_bootstrap_children: 100,
            max_ask_blocks_per_message: 10,
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
        NetworkCommand::AskForBlocks { list } => Some(list),
        _ => None,
    };
    let list = network_controller
        .wait_command(1000.into(), ask_for_block_cmd_filter)
        .await
        .expect("Hash not asked for before timer.");

    assert!(list.get(&node_id).unwrap().contains(&hash_1));
}

pub async fn asked_list(
    network_controller: &mut MockNetworkController,
) -> HashMap<NodeId, Vec<Hash>> {
    let ask_for_block_cmd_filter = |cmd| match cmd {
        NetworkCommand::AskForBlocks { list } => Some(list),
        _ => None,
    };
    network_controller
        .wait_command(1000.into(), ask_for_block_cmd_filter)
        .await
        .expect("Hash not asked for before timer.")
}

pub async fn assert_banned_node(node_id: NodeId, network_controller: &mut MockNetworkController) {
    let banned_node = network_controller
        .wait_command(1000.into(), |cmd| match cmd {
            NetworkCommand::Ban(node) => Some(node),
            _ => None,
        })
        .await
        .expect("Node not banned before timeout.");
    assert_eq!(banned_node, node_id);
}

pub async fn assert_banned_nodes(
    mut nodes: Vec<NodeId>,
    network_controller: &mut MockNetworkController,
) {
    let timer = sleep(UTime::from(5000).into());
    tokio::pin!(timer);
    loop {
        tokio::select! {
            msg = network_controller
                   .wait_command(1000.into(), |cmd| match cmd {
                       NetworkCommand::Ban(node) => Some(node),
                       _ => None,
                   })
             =>  {
                 let banned_node = msg.expect("Nodes not banned before timeout.");
                 nodes.drain_filter(|id| *id == banned_node);
                 if nodes.is_empty() {
                     break;
                 }
            },
            _ = &mut timer => panic!("Nodes not banned before timeout.")
        }
    }
}
