// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::time::Duration;

use massa_consensus_exports::MockConsensusController;
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_pool_exports::MockPoolController;
use massa_pos_exports::MockSelectorController;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;

use crate::{
    handlers::{
        block_handler::{BlockInfoReply, BlockMessage},
        operation_handler::OperationMessage,
    },
    messages::Message,
};

use super::{context::protocol_test, tools::assert_hash_asked_to_node};

#[test]
fn test_protocol_sends_valid_operations_it_receives_to_pool() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = tools::create_operation_with_expire_period(&block_creator, 1);
    let operation_2 = tools::create_operation_with_expire_period(&block_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .returning(move |storage| {
                let storage_operations = storage.get_op_refs();
                assert_eq!(storage_operations.len(), 2);
                assert!(storage_operations.contains(&operation_1.id));
                assert!(storage_operations.contains(&operation_2.id));
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
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![
                        operation_1.clone(),
                        operation_2.clone(),
                    ])),
                )
                .unwrap();
        },
    )
}

#[test]
fn test_protocol_does_not_send_invalid_operations_it_receives_to_pool() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let mut operation_1 = tools::create_operation_with_expire_period(&op_creator, 1);
    // Making the signature of the op invalid
    operation_1.content_creator_pub_key = KeyPair::generate(0).unwrap().get_public_key();
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_clone_box()
        .returning(move || Box::new(MockPoolController::new()));
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation_1])),
                )
                .unwrap();
        },
    )
}

#[test]
fn test_protocol_propagates_operations_to_active_nodes() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .returning(move |storage| {
                let storage_operations = storage.get_op_refs();
                assert_eq!(storage_operations.len(), 1);
                assert!(storage_operations.contains(&operation.id));
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
        move |mut network_controller, mut storage, protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //3. Ask the protocol to propagate the operations.
            storage.store_operations(vec![operation.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            //4. Verify that the operations have been sent to node B only.
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("Node A should not have received the operation.");
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B should have received the operation.");
            match msg {
                Message::Operation(OperationMessage::OperationsAnnouncement(operations)) => {
                    assert_eq!(operations.len(), 1);
                    assert_eq!(
                        operations.iter().next().unwrap(),
                        &operation.id.into_prefix()
                    );
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}

#[test]
fn test_protocol_propagates_operations_received_over_the_network_only_to_nodes_that_dont_know_about_it(
) {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .returning(move |storage| {
                let storage_operations = storage.get_op_refs();
                assert_eq!(storage_operations.len(), 1);
                assert!(storage_operations.contains(&operation.id));
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
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //3. Verify that the operations have been sent to node B only.
            let _ = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect_err("Node A should not have received the operation.");
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B should have received the operation.");
            match msg {
                Message::Operation(OperationMessage::OperationsAnnouncement(operations)) => {
                    assert_eq!(operations.len(), 1);
                    assert_eq!(
                        operations.iter().next().unwrap(),
                        &operation.id.into_prefix()
                    );
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}

#[test]
fn test_protocol_batches_propagation_of_operations_received_over_the_network_and_from_the_api() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let api_operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .returning(move |storage| {
                let storage_operations = storage.get_op_refs();
                assert_eq!(storage_operations.len(), 1);
                assert!(storage_operations.contains(&operation.id));
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
        move |mut network_controller, mut storage, protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            // 3. Send it via the API.
            storage.store_operations(vec![api_operation.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            //4. Verify that one operation have been sent to node A and two to B in one batch.
            let msg = node_a
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node A should have received the operation.");
            match msg {
                Message::Operation(OperationMessage::OperationsAnnouncement(operations)) => {
                    assert_eq!(operations.len(), 1);
                    assert_eq!(
                        operations.iter().next().unwrap(),
                        &api_operation.id.into_prefix()
                    );
                }
                _ => panic!("Unexpected message type."),
            }
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B should have received the operations.");
            match msg {
                Message::Operation(OperationMessage::OperationsAnnouncement(operations)) => {
                    assert_eq!(operations.len(), 2);
                    assert!(operations.contains(&operation.id.into_prefix()));
                    assert!(operations.contains(&api_operation.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}

#[test]
fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header(
) {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let block_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&block_creator, 1);
    let block = tools::create_block_with_operations(
        &block_creator,
        Slot::new(1, 1),
        vec![operation.clone()],
    );
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(move || Box::new(MockConsensusController::new()));
    consensus_controller
        .expect_register_block_header()
        .returning(move |block_id, block| {
            assert_eq!(block_id, block.id);
        });
    consensus_controller.expect_register_block().returning(
        move |block_id, slot, block_storage, created| {
            assert_eq!(block_id, block.id);
            assert_eq!(slot, block.content.header.content.slot);
            let storage_block = block_storage.get_block_refs();
            assert_eq!(storage_block.len(), 1);
            assert!(storage_block.contains(&block.id));
            let storage_ops = block_storage.get_op_refs();
            assert_eq!(storage_ops.len(), 1);
            assert!(storage_ops.contains(&operation.id));
            assert!(!created);
        },
    );
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_clone_box()
        .returning(move || Box::new(MockPoolController::new()));
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, mut storage, protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send the block
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
                )
                .unwrap();

            // 3. Send wishlist
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            assert_hash_asked_to_node(&node_a, &block.id);

            // Node 2 sends block info with ops list, resulting in protocol using the info to determine
            // the node knows about the operations contained in the block.
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::DataResponse {
                        block_id: block.id,
                        block_info: BlockInfoReply::OperationIds(
                            vec![operation.id].into_iter().collect(),
                        ),
                    })),
                )
                .unwrap();

            assert_hash_asked_to_node(&node_a, &block.id);
            storage.store_operations(vec![operation.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            //4. Verify that one operation have been sent to node A and two to B in one batch.
            let _ = node_a
                .recv_timeout(Duration::from_millis(1000))
                .expect_err("Node A shouldn't receive the operation.");
            let msg = node_b
                .recv_timeout(Duration::from_millis(1500))
                .expect("Node B should have received the operations.");
            match msg {
                Message::Operation(OperationMessage::OperationsAnnouncement(operations)) => {
                    assert_eq!(operations.len(), 1);
                    assert!(operations.contains(&operation.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}

#[test]
fn test_protocol_ask_operations_on_batch_received() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_clone_box()
        .returning(move || Box::new(MockPoolController::new()));
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));

            //3. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //4. Node A receive an ask for ops
            let msg = node_a
                .recv_timeout(Duration::from_millis(1000))
                .expect("Node A should have received the ask for ops.");
            match msg {
                Message::Operation(OperationMessage::AskForOperations(asked_operations)) => {
                    assert_eq!(asked_operations.len(), 1);
                    assert!(asked_operations.contains(&operation.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}

#[test]
fn test_protocol_re_ask_operations_to_another_node_on_batch_received_after_delay() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller
        .expect_clone_box()
        .returning(move || Box::new(MockPoolController::new()));
    let mut selector_controller = Box::new(MockSelectorController::new());
    selector_controller
        .expect_clone_box()
        .returning(|| Box::new(MockSelectorController::new()));
    protocol_test(
        &protocol_config,
        consensus_controller,
        pool_controller,
        selector_controller,
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //3. Node A receive an ask for ops
            let msg = node_a
                .recv_timeout(Duration::from_millis(1000))
                .expect("Node A should have received the ask for ops.");
            match msg {
                Message::Operation(OperationMessage::AskForOperations(asked_operations)) => {
                    assert_eq!(asked_operations.len(), 1);
                    assert!(asked_operations.contains(&operation.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }

            //4. Node B send the ops
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //5. Node B receive an ask for ops
            let msg = node_b
                .recv_timeout(Duration::from_millis(1000))
                .expect("Node A should have received the ask for ops.");
            match msg {
                Message::Operation(OperationMessage::AskForOperations(asked_operations)) => {
                    assert_eq!(asked_operations.len(), 1);
                    assert!(asked_operations.contains(&operation.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}

#[test]
fn test_protocol_does_not_re_ask_operations_to_another_node_if_received() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .returning(move |storage| {
                let storage_operations = storage.get_op_refs();
                assert_eq!(storage_operations.len(), 1);
                assert!(storage_operations.contains(&operation.id));
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
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //3. Node A receive an ask for ops
            let msg = node_a
                .recv_timeout(Duration::from_millis(1000))
                .expect("Node A should have received the ask for ops.");
            match msg {
                Message::Operation(OperationMessage::AskForOperations(asked_operations)) => {
                    assert_eq!(asked_operations.len(), 1);
                    assert!(asked_operations.contains(&operation.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }

            //4. Node A send the full ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //5. Node B send the ops
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //6. Node B doesn't receive an ask for ops
            let _ = node_b
                .recv_timeout(Duration::from_millis(1000))
                .expect_err("Node B shouldn't have received the ask for ops.");
        },
    )
}

#[test]
fn test_protocol_on_ask_operations() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);
    let mut consensus_controller = Box::new(MockConsensusController::new());
    consensus_controller
        .expect_clone_box()
        .returning(|| Box::new(MockConsensusController::new()));
    let mut pool_controller = Box::new(MockPoolController::new());
    pool_controller.expect_clone_box().returning(move || {
        let mut pool_controller = Box::new(MockPoolController::new());
        pool_controller
            .expect_add_operations()
            .returning(move |storage| {
                let storage_operations = storage.get_op_refs();
                assert_eq!(storage_operations.len(), 1);
                assert!(storage_operations.contains(&operation.id));
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
        move |mut network_controller, _storage, _protocol_controller| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));

            //2. Node A send full the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //3. Node B send the ask for ops
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Operation(OperationMessage::AskForOperations(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //4. Node B receive the ops
            let msg = node_b
                .recv_timeout(Duration::from_millis(1000))
                .expect("Node B should have received the ops.");
            match msg {
                Message::Operation(OperationMessage::Operations(operations)) => {
                    assert_eq!(operations.len(), 1);
                    assert_eq!(operations[0].id, operation.id);
                }
                _ => panic!("Unexpected message type."),
            }
        },
    )
}
