// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::time::Duration;

use massa_consensus_exports::test_exports::MockConsensusControllerMessage;
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_pool_exports::test_exports::MockPoolControllerMessage;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_time::MassaTime;
use serial_test::serial;

use crate::{
    handlers::{
        block_handler::{BlockInfoReply, BlockMessage},
        operation_handler::OperationMessage,
    },
    messages::Message,
};

use super::{
    context::{protocol_test, protocol_test_with_storage},
    tools::assert_hash_asked_to_node,
};

#[test]
#[serial]
fn test_protocol_sends_valid_operations_it_receives_to_pool() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            //2. Creates 2 ops
            let operation_1 = tools::create_operation_with_expire_period(&node_a_keypair, 1);
            let operation_2 = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![
                        operation_1.clone(),
                        operation_2.clone(),
                    ])),
                )
                .unwrap();

            //4. Check protocol sends operations to pool.
            let received_operations =
                match pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                    evt @ MockPoolControllerMessage::AddOperations { .. } => Some(evt),
                    _ => None,
                }) {
                    Some(MockPoolControllerMessage::AddOperations { operations, .. }) => operations,
                    _ => panic!("Unexpected or no protocol pool event."),
                };

            let op_refs = received_operations.get_op_refs();
            // Check the event includes the expected operations.
            assert!(op_refs.contains(&operation_1.id));
            assert!(op_refs.contains(&operation_2.id));

            let ops_reader = received_operations.read_operations();
            // Check that the operations come with their serialized representations.
            assert_eq!(operation_1.id, ops_reader.get(&operation_1.id).unwrap().id);
            assert_eq!(operation_2.id, ops_reader.get(&operation_2.id).unwrap().id);
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_does_not_send_invalid_operations_it_receives_to_pool() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            //2. Creates 1 op
            let mut operation_1 = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Making the signature of the op invalid
            operation_1.content_creator_pub_key = KeyPair::generate(0).unwrap().get_public_key();

            //4. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation_1])),
                )
                .unwrap();

            //5. Check protocol didn't sent operations to pool.
            match pool_event_receiver.wait_command(1000.into(), |_| Some(())) {
                Some(_) => panic!("Unexpected or no protocol pool event."),
                _ => (),
            }

            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_propagates_operations_to_active_nodes() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver,
              mut storage| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 ops
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //4. Check protocol sends operations to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddOperations { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            });

            //5. Ask the protocol to propagate the operations.
            storage.store_operations(vec![operation.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            //6. Verify that the operations have been sent to node B only.
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
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
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
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 ops
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //4. Check protocol sends operations to pool.
            pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddOperations { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            });

            //5. Verify that the operations have been sent to node B only.
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
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_batches_propagation_of_operations_received_over_the_network_and_from_the_api() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              mut pool_event_receiver,
              mut storage| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 ops
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Node A send the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //4. Check protocol sends operations to pool.
            match pool_event_receiver.wait_command(1000.into(), |evt| match evt {
                MockPoolControllerMessage::AddOperations { .. } => {
                    Some(MockPoolControllerMessage::Any)
                }
                _ => panic!("Unexpected or no protocol pool event."),
            }) {
                Some(_) => {}
                None => panic!("Pool event not received."),
            };

            // 5. Create another operation
            let api_operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            // 6. Send it via the API.
            storage.store_operations(vec![api_operation.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            //7. Verify that one operation have been sent to node A and two to B in one batch.
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
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
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
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              mut consensus_event_receiver,
              pool_event_receiver,
              mut storage| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (_node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 ops
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Create a block from the operation
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, 1),
                vec![operation.clone()],
            );

            //4. Node A send the block
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            //5. Assert that we register the block header to the consensus
            loop {
                match consensus_event_receiver.wait_command(
                    MassaTime::from_millis(100),
                    |command| match command {
                        MockConsensusControllerMessage::RegisterBlockHeader {
                            header,
                            block_id,
                        } => {
                            assert_eq!(header.id, block.content.header.id);
                            assert_eq!(block_id, block.id);
                            Some(())
                        }
                        _evt => None,
                    },
                ) {
                    Some(()) => {
                        break;
                    }
                    None => {
                        continue;
                    }
                }
            }

            // 6. Send wishlist
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
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![operation.id].into_iter().collect()),
                    )]))),
                )
                .unwrap();

            assert_hash_asked_to_node(&node_a, &block.id);
            storage.store_operations(vec![operation.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            //7. Verify that one operation have been sent to node A and two to B in one batch.
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
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_wrong_root_hash_header(
) {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test_with_storage(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver,
              mut storage| {
            //1. Create 3 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let node_c_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            let (node_c_peer_id, node_c) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_c_keypair.get_public_key()));
            //2. Creates 2 ops
            let operation_1 = tools::create_operation_with_expire_period(&node_a_keypair, 1);
            let operation_2 = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Create a block from the operation
            let block = tools::create_block_with_operations(
                &node_a_keypair,
                Slot::new(1, 1),
                vec![operation_1.clone()],
            );

            //4. Node A send the block
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Block(Box::new(BlockMessage::BlockHeader(
                        block.content.header.clone(),
                    ))),
                )
                .unwrap();

            // 5. Send wishlist
            protocol_controller
                .send_wishlist_delta(
                    vec![(block.id, Some(block.content.header.clone()))]
                        .into_iter()
                        .collect(),
                    PreHashSet::<BlockId>::default(),
                )
                .unwrap();

            assert_hash_asked_to_node(&node_a, &block.id);
            assert_hash_asked_to_node(&node_b, &block.id);
            assert_hash_asked_to_node(&node_c, &block.id);

            //6. Node B sends block info with bad ops list. Making him ban
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![operation_2.id].into_iter().collect()),
                    )]))),
                )
                .unwrap();

            //7. Node C sends block info with right ops list
            network_controller
                .send_from_peer(
                    &node_c_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Info(vec![operation_1.id].into_iter().collect()),
                    )]))),
                )
                .unwrap();

            assert_hash_asked_to_node(&node_c, &block.id);
            network_controller
                .send_from_peer(
                    &node_c_peer_id,
                    Message::Block(Box::new(BlockMessage::ReplyForBlocks(vec![(
                        block.id,
                        BlockInfoReply::Operations(vec![operation_1]),
                    )]))),
                )
                .unwrap();
            //8. Propagate operations that is not in the block and so should be propagated to everyone
            storage.store_operations(vec![operation_2.clone()]);
            protocol_controller.propagate_operations(storage).unwrap();

            let msgs = (
                node_a
                    .recv_timeout(Duration::from_millis(1000))
                    .expect("Node B should have received the operations."),
                node_b
                    .recv_timeout(Duration::from_millis(1000))
                    .expect_err("Node B should not have received the operations."),
                node_c
                    .recv_timeout(Duration::from_millis(1000))
                    .expect("Node B should have received the operations."),
            );
            match msgs {
                (
                    Message::Operation(OperationMessage::OperationsAnnouncement(operations)),
                    _,
                    Message::Operation(OperationMessage::OperationsAnnouncement(operations3)),
                ) => {
                    assert_eq!(operations.len(), 2);
                    assert!(operations.contains(&operation_2.id.into_prefix()));
                    assert_eq!(operations3.len(), 1);
                    assert!(operations3.contains(&operation_2.id.into_prefix()));
                }
                _ => panic!("Unexpected message type."),
            }
            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_ask_operations_on_batch_received() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 1 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            //2. Creates 1 op
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

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

            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_re_ask_operations_to_another_node_on_batch_received_after_delay() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 op
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

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

            //5. Node B send the ops
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //4. Node B receive an ask for ops
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

            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_does_not_re_ask_operations_to_another_node_if_received() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 op
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

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

            //5. Node A send the full ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //6. Node B send the ops
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Operation(OperationMessage::OperationsAnnouncement(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //7. Node B doesn't receive an ask for ops
            let _ = node_b
                .recv_timeout(Duration::from_millis(1000))
                .expect_err("Node B shouldn't have received the ask for ops.");

            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}

#[test]
#[serial]
fn test_protocol_on_ask_operations() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default_panic(info);
        std::process::exit(1);
    }));

    let mut protocol_config = ProtocolConfig::default();
    protocol_config.thread_count = 2;
    protocol_config.initial_peers = "./src/tests/empty_initial_peers.json".to_string().into();
    protocol_test(
        &protocol_config,
        move |mut network_controller,
              protocol_controller,
              protocol_manager,
              consensus_event_receiver,
              pool_event_receiver| {
            //1. Create 2 node
            let node_a_keypair = KeyPair::generate(0).unwrap();
            let (node_a_peer_id, _node_a) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_a_keypair.get_public_key()));
            let node_b_keypair = KeyPair::generate(0).unwrap();
            let (node_b_peer_id, node_b) = network_controller
                .create_fake_connection(PeerId::from_public_key(node_b_keypair.get_public_key()));
            //2. Creates 1 op
            let operation = tools::create_operation_with_expire_period(&node_a_keypair, 1);

            //3. Node A send full the ops
            network_controller
                .send_from_peer(
                    &node_a_peer_id,
                    Message::Operation(OperationMessage::Operations(vec![operation.clone()])),
                )
                .unwrap();

            //4. Node B send the ask for ops
            network_controller
                .send_from_peer(
                    &node_b_peer_id,
                    Message::Operation(OperationMessage::AskForOperations(
                        vec![operation.id.into_prefix()].into_iter().collect(),
                    )),
                )
                .unwrap();

            //5. Node B receive the ops
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

            (
                network_controller,
                protocol_controller,
                protocol_manager,
                consensus_event_receiver,
                pool_event_receiver,
            )
        },
    )
}
