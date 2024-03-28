// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::HashSet;
use std::time::Duration;

use massa_models::config::CHAINID;
use massa_models::operation::{OperationPrefixId, SecureShareOperation};
use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::ProtocolConfig;
use massa_signature::KeyPair;
use massa_test_framework::{TestUniverse, WaitPoint};
use massa_time::MassaTime;
use mockall::{predicate, Sequence};

use crate::handlers::block_handler::AskForBlockInfo;
use crate::wrap_network::MockActiveConnectionsTraitWrapper;
use crate::{
    handlers::{
        block_handler::{BlockInfoReply, BlockMessage},
        operation_handler::OperationMessage,
    },
    messages::Message,
};

use super::universe::{ProtocolForeignControllers, ProtocolTestUniverse};

enum TestsStepMatch {
    // bool for sequence or not
    OperationsPropagated((PeerId, Vec<OperationPrefixId>, bool)),
    AskBlockInfos((PeerId, BlockId, AskForBlockInfo)),
    AskForOperations((PeerId, Vec<OperationPrefixId>)),
    OperationsInPool(Vec<SecureShareOperation>),
    OperationsSent((PeerId, Vec<SecureShareOperation>)),
}

/// This function allow you to pass a sequence of steps that should be executed in an certain order.
/// This will be checked and after each step executed the waitpoint will be triggered.
fn operation_workflow_mock(
    steps: Vec<TestsStepMatch>,
    foreign_controllers: &mut ProtocolForeignControllers,
    waitpoint_trigger_handle: WaitPoint,
) {
    let mut sequence = Sequence::new();
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    let mut peer_ids = HashSet::new();
    for step in steps {
        let waitpoint_trigger_handle = waitpoint_trigger_handle.get_trigger_handle();
        match step {
            TestsStepMatch::OperationsPropagated((node_peer_id, propagated_operations, seq)) => {
                peer_ids.insert(node_peer_id);
                shared_active_connections.set_expectations(|active_connections| {
                    let expect = active_connections.expect_send_to_peer().times(1);
                    if seq {
                        expect.in_sequence(&mut sequence);
                    }
                    expect
                        .with(
                            predicate::eq(node_peer_id),
                            predicate::always(),
                            predicate::always(),
                            predicate::always(),
                        )
                        .returning(move |_, _, message, high_priority| {
                            assert!(!high_priority);
                            match message {
                                Message::Operation(OperationMessage::OperationsAnnouncement(
                                    operations,
                                )) => {
                                    assert_eq!(operations.len(), propagated_operations.len());
                                    for op in propagated_operations.iter() {
                                        assert!(operations.contains(op));
                                    }
                                    waitpoint_trigger_handle.trigger();
                                }
                                _ => panic!("Unexpected message type."),
                            }
                            Ok(())
                        });
                });
            }
            TestsStepMatch::AskBlockInfos((node_peer_id, asked_block_id, asked_block_info)) => {
                peer_ids.insert(node_peer_id);
                shared_active_connections.set_expectations(|active_connections| {
                    active_connections
                        .expect_send_to_peer()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(move |peer_id, _, message, high_priority| {
                            assert_eq!(*peer_id, node_peer_id);
                            assert!(high_priority);
                            match message {
                                Message::Block(message) => match *message {
                                    BlockMessage::DataRequest {
                                        block_id,
                                        block_info,
                                    } => {
                                        assert_eq!(block_id, asked_block_id);
                                        assert_eq!(block_info, asked_block_info);
                                        waitpoint_trigger_handle.trigger();
                                        Ok(())
                                    }
                                    _ => panic!("Unexpected message type."),
                                },
                                _ => panic!("Unexpected message type."),
                            }
                        });
                });
            }
            TestsStepMatch::AskForOperations((node_peer_id, asked_op_ids)) => {
                peer_ids.insert(node_peer_id);
                shared_active_connections.set_expectations(|active_connections| {
                    active_connections
                        .expect_send_to_peer()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(move |peer_id, _, message, high_priority| {
                            assert_eq!(*peer_id, node_peer_id);
                            assert!(!high_priority);
                            match message {
                                Message::Operation(OperationMessage::AskForOperations(
                                    operations,
                                )) => {
                                    assert_eq!(operations.len(), asked_op_ids.len());
                                    for op in asked_op_ids.iter() {
                                        assert!(operations.contains(op));
                                    }
                                    waitpoint_trigger_handle.trigger();
                                }
                                _ => panic!("Unexpected message type."),
                            }
                            Ok(())
                        });
                });
            }
            TestsStepMatch::OperationsSent((node_peer_id, sent_operations)) => {
                peer_ids.insert(node_peer_id);
                shared_active_connections.set_expectations(|active_connections| {
                    active_connections
                        .expect_send_to_peer()
                        .times(1)
                        .in_sequence(&mut sequence)
                        .returning(move |peer_id, _, message, high_priority| {
                            assert_eq!(*peer_id, node_peer_id);
                            assert!(!high_priority);
                            match message {
                                Message::Operation(OperationMessage::Operations(operations)) => {
                                    assert_eq!(operations.len(), sent_operations.len());
                                    waitpoint_trigger_handle.trigger();
                                }
                                _ => panic!("Unexpected message type."),
                            }
                            Ok(())
                        });
                });
            }
            TestsStepMatch::OperationsInPool(operations) => {
                foreign_controllers
                    .pool_controller
                    .set_expectations(|pool_controller| {
                        pool_controller
                            .expect_add_operations()
                            .times(1)
                            .in_sequence(&mut sequence)
                            .returning(move |op_storage| {
                                let storage_operations = op_storage.get_op_refs();
                                assert_eq!(storage_operations.len(), operations.len());
                                for op in operations.iter() {
                                    assert!(storage_operations.contains(&op.id));
                                }
                                waitpoint_trigger_handle.trigger();
                            });
                    });
            }
        }
    }
    ProtocolTestUniverse::active_connections_boilerplate(&mut shared_active_connections, peer_ids);
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
}

#[test]
fn test_protocol_sends_valid_operations_it_receives_to_pool() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![TestsStepMatch::OperationsInPool(vec![
            operation_1.clone(),
            operation_2.clone(),
        ])],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![
            operation_1.clone(),
            operation_2.clone(),
        ])),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_does_not_send_invalid_operations_it_receives_to_pool() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let mut operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    operation_1.content_creator_pub_key = KeyPair::generate(0).unwrap().get_public_key();
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let waitpoint_trigger_handle2 = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .times(1)
        .returning(move |peer_id| {
            assert_eq!(*peer_id, node_a_peer_id);
            waitpoint_trigger_handle2.trigger();
        });
    operation_workflow_mock(vec![], &mut foreign_controllers, waitpoint_trigger_handle);
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![
            operation_1.clone(),
            operation_2.clone(),
        ])),
    );
    //Peer ban
    waitpoint.wait();
}

#[test]
fn test_protocol_propagates_operations_to_active_nodes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![
            TestsStepMatch::OperationsInPool(vec![operation_1.clone(), operation_2.clone()]),
            TestsStepMatch::OperationsPropagated((
                node_b_peer_id,
                vec![operation_1.clone(), operation_2.clone()]
                    .into_iter()
                    .map(|op| op.id.into_prefix())
                    .collect(),
                true,
            )),
        ],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![
            operation_1.clone(),
            operation_2.clone(),
        ])),
    );
    waitpoint.wait();
    waitpoint.wait();
}

#[test]
fn test_protocol_batches_propagation_of_operations_received_over_the_network_and_from_the_api() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![
            TestsStepMatch::OperationsInPool(vec![operation_1.clone()]),
            TestsStepMatch::OperationsPropagated((
                node_b_peer_id,
                vec![operation_1.clone(), operation_2.clone()]
                    .into_iter()
                    .map(|op| op.id.into_prefix())
                    .collect(),
                true,
            )),
        ],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![operation_1.clone()])),
    );
    universe.storage.store_operations(vec![operation_2.clone()]);
    universe
        .module_controller
        .propagate_operations(universe.storage.clone())
        .unwrap();
    waitpoint.wait();
    waitpoint.wait();
}

#[test]
fn test_protocol_propagates_operations_only_to_nodes_that_dont_know_about_it_indirect_knowledge_via_header(
) {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(100),
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let op_thread = operation_1
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, op_thread),
        vec![operation_1.clone()],
        vec![],
        vec![],
    );
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let waitpoint_trigger_handle2 = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .times(1)
        .returning(move |_, _| {});
    foreign_controllers
        .consensus_controller
        .expect_register_block()
        .times(1)
        .returning(move |_, _, _, _| {
            waitpoint_trigger_handle2.trigger();
        });
    operation_workflow_mock(
        vec![
            TestsStepMatch::AskBlockInfos((
                node_a_peer_id,
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskBlockInfos((
                node_a_peer_id,
                block.id,
                AskForBlockInfo::Operations(vec![operation_1.id]),
            )),
            TestsStepMatch::OperationsInPool(vec![operation_1.clone()]),
            TestsStepMatch::OperationsPropagated((
                node_a_peer_id,
                vec![operation_2.clone()]
                    .into_iter()
                    .map(|op| op.id.into_prefix())
                    .collect(),
                false,
            )),
            TestsStepMatch::OperationsPropagated((
                node_b_peer_id,
                vec![operation_1.clone(), operation_2.clone()]
                    .into_iter()
                    .map(|op| op.id.into_prefix())
                    .collect(),
                false,
            )),
        ],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );
    universe
        .module_controller
        .send_wishlist_delta(
            vec![(block.id, Some(block.content.header.clone()))]
                .into_iter()
                .collect(),
            PreHashSet::<BlockId>::default(),
        )
        .unwrap();
    waitpoint.wait();

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![operation_1.id].into_iter().collect()),
        })),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::Operations(vec![operation_1].into_iter().collect()),
        })),
    );
    waitpoint.wait();

    universe.storage.store_operations(vec![operation_2.clone()]);
    universe
        .module_controller
        .propagate_operations(universe.storage.clone())
        .unwrap();

    waitpoint.wait();
    waitpoint.wait();
}

#[test]
fn test_protocol_ask_operations_on_batch_received() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let op_prefixes = vec![operation_1.id.into_prefix(), operation_2.id.into_prefix()];

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![TestsStepMatch::AskForOperations((
            node_a_peer_id,
            op_prefixes.clone(),
        ))],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::OperationsAnnouncement(
            op_prefixes.into_iter().collect(),
        )),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_re_ask_operations_to_another_node_on_batch_received_after_delay() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let op_prefixes = vec![operation_1.id.into_prefix(), operation_2.id.into_prefix()];

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![
            TestsStepMatch::AskForOperations((node_a_peer_id, op_prefixes.clone())),
            TestsStepMatch::AskForOperations((node_b_peer_id, op_prefixes.clone())),
        ],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::OperationsAnnouncement(
            op_prefixes.clone().into_iter().collect(),
        )),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Operation(OperationMessage::OperationsAnnouncement(
            op_prefixes.into_iter().collect(),
        )),
    );
    waitpoint.wait();
}

#[test]
fn test_protocol_does_not_re_ask_operations_to_another_node_if_received() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let op_prefixes = vec![operation_1.id.into_prefix(), operation_2.id.into_prefix()];

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![
            TestsStepMatch::AskForOperations((node_a_peer_id, op_prefixes.clone())),
            TestsStepMatch::OperationsInPool(vec![operation_1.clone(), operation_2.clone()]),
        ],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::OperationsAnnouncement(
            op_prefixes.clone().into_iter().collect(),
        )),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![
            operation_1.clone(),
            operation_2.clone(),
        ])),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Operation(OperationMessage::OperationsAnnouncement(
            op_prefixes.into_iter().collect(),
        )),
    );

    //TODO: Find a way to test we didn't sent
    std::thread::sleep(Duration::from_millis(200));
}

#[test]
fn test_protocol_on_ask_operations() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let op_prefixes = vec![operation_1.id.into_prefix(), operation_2.id.into_prefix()];

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    operation_workflow_mock(
        vec![
            TestsStepMatch::OperationsInPool(vec![operation_1.clone(), operation_2.clone()]),
            TestsStepMatch::OperationsSent((
                node_b_peer_id,
                vec![operation_1.clone(), operation_2.clone()],
            )),
        ],
        &mut foreign_controllers,
        waitpoint_trigger_handle,
    );
    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![
            operation_1.clone(),
            operation_2.clone(),
        ])),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Operation(OperationMessage::AskForOperations(
            op_prefixes.into_iter().collect(),
        )),
    );
    waitpoint.wait();
}
