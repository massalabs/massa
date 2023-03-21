// Copyright (c) 2022 MASSA LABS <info@massa.net>

use super::mock_network_controller::MockNetworkController;
use crate::ProtocolConfig;
use massa_hash::Hash;
use massa_models::node::NodeId;
use massa_models::operation::OperationSerializer;
use massa_models::secure_share::SecureShareContent;
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockSerializer, SecureShareBlock},
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    endorsement::{Endorsement, EndorsementSerializerLW, SecureShareEndorsement},
    operation::{Operation, OperationType, SecureShareOperation},
    slot::Slot,
};
use massa_network_exports::{AskForBlocksInfo, NetworkCommand};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use std::collections::HashMap;
use tokio::time::sleep;

/// test utility structures
/// keeps keypair and associated node id
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// key pair of the node
    pub keypair: KeyPair,
    /// node id
    pub id: NodeId,
}

/// create node info
pub fn create_node() -> NodeInfo {
    let keypair = KeyPair::generate();
    let id = NodeId::new(keypair.get_public_key());
    NodeInfo { keypair, id }
}

/// create number of nodes and connect them with protocol
pub async fn create_and_connect_nodes(
    num: usize,
    network_controller: &mut MockNetworkController,
) -> Vec<NodeInfo> {
    let mut nodes = vec![];
    for _ in 0..num {
        // Create a node, and connect it with protocol.
        let info = create_node();
        network_controller.new_connection(info.id).await;
        nodes.push(info);
    }
    nodes
}

/// Creates a block for use in protocol,
/// without paying attention to consensus related things
/// like slot, parents, and merkle root.
pub fn create_block(keypair: &KeyPair) -> SecureShareBlock {
    let header = BlockHeader::new_verifiable(
        BlockHeader {
            slot: Slot::new(1, 0),
            parents: vec![
                BlockId(Hash::compute_from("Genesis 0".as_bytes())),
                BlockId(Hash::compute_from("Genesis 1".as_bytes())),
            ],
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        keypair,
    )
    .unwrap();

    Block::new_verifiable(
        Block {
            header,
            operations: Default::default(),
        },
        BlockSerializer::new(),
        keypair,
    )
    .unwrap()
}

/// create a block with no endorsement
///
/// * `keypair`: key that sign the block
/// * `slot`
/// * `operations`
pub fn create_block_with_operations(
    keypair: &KeyPair,
    slot: Slot,
    operations: Vec<SecureShareOperation>,
) -> SecureShareBlock {
    let operation_merkle_root = Hash::compute_from(
        &operations.iter().fold(Vec::new(), |acc, v| {
            [acc, v.id.to_bytes().to_vec()].concat()
        })[..],
    );
    let header = BlockHeader::new_verifiable(
        BlockHeader {
            slot,
            parents: vec![
                BlockId(Hash::compute_from("Genesis 0".as_bytes())),
                BlockId(Hash::compute_from("Genesis 1".as_bytes())),
            ],
            operation_merkle_root,
            endorsements: Vec::new(),
        },
        BlockHeaderSerializer::new(),
        keypair,
    )
    .unwrap();

    let op_ids = operations.into_iter().map(|op| op.id).collect();
    Block::new_verifiable(
        Block {
            header,
            operations: op_ids,
        },
        BlockSerializer::new(),
        keypair,
    )
    .unwrap()
}

/// create a block with no operation
///
/// * `keypair`: key that sign the block
/// * `slot`
/// * `endorsements`
pub fn create_block_with_endorsements(
    keypair: &KeyPair,
    slot: Slot,
    endorsements: Vec<SecureShareEndorsement>,
) -> SecureShareBlock {
    let header = BlockHeader::new_verifiable(
        BlockHeader {
            slot,
            parents: vec![
                BlockId(Hash::compute_from("Genesis 0".as_bytes())),
                BlockId(Hash::compute_from("Genesis 1".as_bytes())),
            ],
            operation_merkle_root: Hash::compute_from(&Vec::new()),
            endorsements,
        },
        BlockHeaderSerializer::new(),
        keypair,
    )
    .unwrap();

    Block::new_verifiable(
        Block {
            header,
            operations: Default::default(),
        },
        BlockSerializer::new(),
        keypair,
    )
    .unwrap()
}

/// Creates an endorsement for use in protocol tests,
/// without paying attention to consensus related things.
pub fn create_endorsement() -> SecureShareEndorsement {
    let keypair = KeyPair::generate();

    let content = Endorsement {
        slot: Slot::new(10, 1),
        index: 0,
        endorsed_block: BlockId(Hash::compute_from(&[])),
    };
    Endorsement::new_verifiable(content, EndorsementSerializerLW::new(), &keypair).unwrap()
}

/// Create an operation, from a specific sender, and with a specific expire period.
pub fn create_operation_with_expire_period(
    keypair: &KeyPair,
    expire_period: u64,
) -> SecureShareOperation {
    let recv_keypair = KeyPair::generate();

    let op = OperationType::Transaction {
        recipient_address: Address::from_public_key(&recv_keypair.get_public_key()),
        amount: Amount::default(),
    };
    let content = Operation {
        fee: Amount::default(),
        op,
        expire_period,
    };
    Operation::new_verifiable(content, OperationSerializer::new(), keypair).unwrap()
}

lazy_static::lazy_static! {
    /// protocol settings
    pub static ref PROTOCOL_CONFIG: ProtocolConfig = create_protocol_config();
}

/// create a `ProtocolConfig` with typical values
pub fn create_protocol_config() -> ProtocolConfig {
    ProtocolConfig {
        ask_block_timeout: 500.into(),
        max_known_blocks_size: 100,
        max_node_known_blocks_size: 100,
        max_node_wanted_blocks_size: 100,
        max_simultaneous_ask_blocks_per_node: 10,
        max_send_wait: MassaTime::from_millis(100),
        max_known_ops_size: 1000,
        max_node_known_ops_size: 1000,
        max_known_endorsements_size: 1000,
        max_node_known_endorsements_size: 1000,
        operation_batch_buffer_capacity: 1000,
        operation_announcement_buffer_capacity: 1000,
        operation_batch_proc_period: 200.into(),
        asked_operations_pruning_period: 500.into(),
        operation_announcement_interval: 150.into(),
        max_operations_per_message: 1024,
        thread_count: 32.try_into().unwrap(),
        max_serialized_operations_size_per_block: 1024,
        controller_channel_size: 1024,
        event_channel_size: 1024,
        genesis_timestamp: MassaTime::now().unwrap(),
        t0: MassaTime::from_millis(16000),
        max_operations_propagation_time: MassaTime::from_millis(30000),
        max_endorsements_propagation_time: MassaTime::from_millis(60000),
    }
}

/// assert block id has been asked to node
pub async fn assert_hash_asked_to_node(
    hash_1: BlockId,
    node_id: NodeId,
    network_controller: &mut MockNetworkController,
) {
    let ask_for_block_cmd_filter = |cmd| match cmd {
        NetworkCommand::AskForBlocks { list } => Some(list),
        _ => None,
    };
    let mut list = network_controller
        .wait_command(1000.into(), ask_for_block_cmd_filter)
        .await
        .expect("Hash not asked for before timer.");

    assert_eq!(list.get_mut(&node_id).unwrap().pop().unwrap().0, hash_1);
}

/// retrieve what blocks where asked to which nodes
pub async fn asked_list(
    network_controller: &mut MockNetworkController,
) -> HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>> {
    let ask_for_block_cmd_filter = |cmd| match cmd {
        NetworkCommand::AskForBlocks { list } => Some(list),
        _ => None,
    };
    network_controller
        .wait_command(1000.into(), ask_for_block_cmd_filter)
        .await
        .expect("Hash not asked for before timer.")
}

/// assert a list of node(s) has been banned
pub async fn assert_banned_nodes(
    mut nodes: Vec<NodeId>,
    network_controller: &mut MockNetworkController,
) {
    let timer = sleep(MassaTime::from_millis(5000).into());
    tokio::pin!(timer);
    loop {
        tokio::select! {
            msg = network_controller
                   .wait_command(2000.into(), |cmd| match cmd {
                       NetworkCommand::NodeBanByIds(node) => Some(node),
                       _ => None,
                   })
             =>  {
                 let banned_nodes = msg.expect("Nodes not banned before timeout.");
                 nodes.drain_filter(|id| banned_nodes.contains(id));
                 if nodes.is_empty() {
                     break;
                 }
            },
            _ = &mut timer => panic!("Nodes not banned before timeout.")
        }
    }
}
