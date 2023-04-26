// Copyright (c) 2022 MASSA LABS <info@massa.net>

use massa_hash::Hash;
use massa_models::endorsement::EndorsementSerializer;
use massa_models::operation::OperationSerializer;
use massa_models::secure_share::SecureShareContent;
use massa_models::{
    address::Address,
    amount::Amount,
    block::{Block, BlockSerializer, SecureShareBlock},
    block_header::{BlockHeader, BlockHeaderSerializer},
    block_id::BlockId,
    endorsement::{Endorsement, SecureShareEndorsement},
    operation::{Operation, OperationType, SecureShareOperation},
    slot::Slot,
};
use massa_signature::KeyPair;

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
    Endorsement::new_verifiable(content, EndorsementSerializer::new(), &keypair).unwrap()
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

// /// retrieve what blocks where asked to which nodes
// pub async fn asked_list(
//     network_controller: &mut MockNetworkController,
// ) -> HashMap<NodeId, Vec<(BlockId, AskForBlocksInfo)>> {
//     let ask_for_block_cmd_filter = |cmd| match cmd {
//         NetworkCommand::AskForBlocks { list } => Some(list),
//         _ => None,
//     };
//     network_controller
//         .wait_command(1000.into(), ask_for_block_cmd_filter)
//         .await
//         .expect("Hash not asked for before timer.")
// }

// /// assert a list of node(s) has been banned
// pub async fn assert_banned_nodes(
//     mut nodes: Vec<NodeId>,
//     network_controller: &mut MockNetworkController,
// ) {
//     let timer = sleep(MassaTime::from_millis(5000).into());
//     tokio::pin!(timer);
//     loop {
//         tokio::select! {
//             msg = network_controller
//                    .wait_command(2000.into(), |cmd| match cmd {
//                        NetworkCommand::NodeBanByIds(node) => Some(node),
//                        _ => None,
//                    })
//              =>  {
//                  let banned_nodes = msg.expect("Nodes not banned before timeout.");
//                  nodes.drain_filter(|id| banned_nodes.contains(id));
//                  if nodes.is_empty() {
//                      break;
//                  }
//             },
//             _ = &mut timer => panic!("Nodes not banned before timeout.")
//         }
//     }
// }
