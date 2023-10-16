// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::HashSet;
use std::time::Duration;

use massa_consensus_exports::MockConsensusController;
use massa_models::operation::OperationId;
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::MockSelectorController;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;

use crate::{
    handlers::block_handler::{AskForBlockInfo, BlockInfoReply, BlockMessage},
    messages::Message,
};

use super::{context::protocol_test, tools::assert_hash_asked_to_node};

#[test]
fn test_noting_block_does_not_panic_with_one_max_node_known_blocks_size() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.max_node_known_blocks_size = 1;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let block_creator = KeyPair::generate(0).unwrap();
    let op_1 = tools::create_operation_with_expire_period(&block_creator, 5);
    let op_2 = tools::create_operation_with_expire_period(&block_creator, 5);
    let op_thread = op_1
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = tools::create_block_with_operations(
        &block_creator,
        Slot::new(1, op_thread),
        vec![op_1.clone(), op_2.clone()],
    );
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    consensus_controller.expect_register_block().return_once(
        move |block_id, slot, storage, created| {
            assert_eq!(block_id, block.id);
            assert_eq!(slot, block.content.header.content.slot);
            let storage_block = storage.get_block_refs();
            assert_eq!(storage_block.len(), 1);
            assert!(storage_block.contains(&block.id));
            let storage_ops = storage.get_op_refs();
            assert_eq!(storage_ops.len(), 2);
            assert!(storage_ops.contains(&op_1.id));
            assert!(storage_ops.contains(&op_2.id));
            assert!(!created);
        },
    );
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .return_once(move |storage_ops| {
                let op_ids = storage_ops.get_op_refs();
                assert_eq!(op_ids.len(), 2);
                assert!(op_ids.contains(&op_1.id));
                assert!(op_ids.contains(&op_2.id));
            });
        pool_controller
    });
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, protocol_controller| {
            //1. Create 2 nodes
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //end setup

            //2. Send the block header from node a
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
                )
                .unwrap();

            //3. Send a wishlist that ask for the block
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            //4. Assert that we asked the block to node a then node b then a again then b
            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);
            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);

            //5. Node B answers with the list of operation IDs
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::DataResponse {
                        block_id: block.id,
                        block_info: BlockInfoReply::OperationIds(vec![op_1.id, op_2.id]),
                    })),
                )
                .unwrap();

            //6. Assert that we asked the operations to node b
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B didn't receive the ask for operations message");
            match msg {
                Message::Block(message) => {
                    if let BlockMessage::DataRequest {
                        block_id,
                        block_info,
                    } = *message
                    {
                        assert_eq!(block_id, block.id);
                        if let AskForBlockInfo::Operations(operations) = block_info {
                            assert_eq!(
                                operations.into_iter().collect::<HashSet<OperationId>>(),
                                vec![op_1.id, op_2.id]
                                    .into_iter()
                                    .collect::<HashSet<OperationId>>()
                            );
                        } else {
                            panic!("Node B didn't receive the ask for operations message");
                        }
                    } else {
                        panic!("Node B didn't receive the ask for operations message");
                    }
                }
                _ => panic!("Node B didn't receive the ask for operations message"),
            }

            //7. Node B answer with the operations
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::DataResponse {
                        block_id: block.id,
                        block_info: BlockInfoReply::Operations(vec![op_1, op_2]),
                    })),
                )
                .unwrap();
        },
    )
}
