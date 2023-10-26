// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::HashSet;

use crate::handlers::block_handler::{AskForBlockInfo, BlockInfoReply, BlockMessage};
use crate::messages::Message;
use crate::wrap_network::MockActiveConnectionsTraitWrapper;

use super::universe::{ProtocolForeignControllers, ProtocolTestUniverse};
use massa_models::block_header::SecuredHeader;
use massa_models::operation::OperationId;
use massa_models::prehash::PreHashSet;
use massa_models::{block_id::BlockId, slot::Slot};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::ProtocolConfig;
use massa_signature::KeyPair;
use massa_test_framework::{TestUniverse, WaitPoint};
use massa_time::MassaTime;
use mockall::Sequence;

#[derive(Clone, Debug)]
enum PeerIdMatchers {
    PeerId(PeerId),
    AmongPeerIds(HashSet<PeerId>),
}

impl PeerIdMatchers {
    fn matches(&self, peer_id: &PeerId) -> bool {
        match self {
            PeerIdMatchers::PeerId(match_peer_id) => match_peer_id == peer_id,
            PeerIdMatchers::AmongPeerIds(match_peer_ids) => match_peer_ids.contains(peer_id),
        }
    }

    fn extend(&self, peer_ids: &mut HashSet<PeerId>) {
        match self {
            PeerIdMatchers::PeerId(match_peer_id) => {
                peer_ids.insert(*match_peer_id);
            }
            PeerIdMatchers::AmongPeerIds(match_peer_ids) => {
                peer_ids.extend(match_peer_ids);
            }
        }
    }
}

#[allow(clippy::large_enum_variant)]
enum TestsStepMatch {
    // Match an ask asking for the block infos and match the block infos asked
    AskData((PeerIdMatchers, BlockId, AskForBlockInfo)),
    // Match a send of a block header and match the block header sent
    SendHeader((PeerIdMatchers, BlockId, SecuredHeader)),
    // Match a send of information and match the block infos sent
    SendData((PeerIdMatchers, BlockId, BlockInfoReply)),
    // Match all expected call when a block is managed (id, has_ops)
    BlockManaged((BlockId, bool)),
}

fn block_retrieval_mock(
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
            TestsStepMatch::AskData((node_peer_id, asked_block_id, asked_infos)) => {
                node_peer_id.extend(&mut peer_ids);
                shared_active_connections.set_expectations({
                    |active_connections| {
                        active_connections
                            .expect_send_to_peer()
                            .times(1)
                            .in_sequence(&mut sequence)
                            .returning(move |peer_id, _, message, high_priority| {
                                assert!(node_peer_id.matches(peer_id));
                                match message {
                                    Message::Block(message) => match *message {
                                        BlockMessage::DataRequest {
                                            block_id,
                                            block_info,
                                        } => {
                                            assert_eq!(block_id, asked_block_id);
                                            assert_eq!(block_info, asked_infos);
                                        }
                                        _ => panic!("Node didn't receive the infos block message"),
                                    },
                                    _ => panic!("Node didn't receive the infos block message"),
                                }
                                assert!(high_priority);
                                waitpoint_trigger_handle.trigger();
                                Ok(())
                            });
                    }
                });
            }
            TestsStepMatch::SendData((node_peer_id, sent_block_id, _sent_infos)) => {
                node_peer_id.extend(&mut peer_ids);
                shared_active_connections.set_expectations({
                    |active_connections| {
                        active_connections
                            .expect_send_to_peer()
                            .times(1)
                            .in_sequence(&mut sequence)
                            .returning(move |peer_id, _, message, high_priority| {
                                std::thread::sleep(std::time::Duration::from_millis(50));
                                assert!(node_peer_id.matches(peer_id));
                                match message {
                                    Message::Block(message) => match *message {
                                        BlockMessage::DataResponse { block_id, .. } => {
                                            assert_eq!(block_id, sent_block_id);
                                            //TODO: CHeck block info, difficult because doesn't implement Eq.
                                        }
                                        _ => panic!("Node didn't receive the infos block message"),
                                    },
                                    _ => panic!("Node didn't receive the infos block message"),
                                }
                                assert!(high_priority);
                                waitpoint_trigger_handle.trigger();
                                Ok(())
                            });
                    }
                });
            }
            TestsStepMatch::SendHeader((node_peer_id, sent_block_id, sent_header)) => {
                node_peer_id.extend(&mut peer_ids);
                shared_active_connections.set_expectations({
                    |active_connections| {
                        active_connections
                            .expect_send_to_peer()
                            .times(1)
                            .in_sequence(&mut sequence)
                            .returning(move |peer_id, _, message, high_priority| {
                                std::thread::sleep(std::time::Duration::from_millis(50));
                                assert!(node_peer_id.matches(peer_id));
                                match message {
                                    Message::Block(message) => match *message {
                                        BlockMessage::Header(header) => {
                                            assert_eq!(header.id, sent_block_id);
                                            assert_eq!(header.id, sent_header.id);
                                        }
                                        _ => panic!("Node didn't receive the infos block message"),
                                    },
                                    _ => panic!("Node didn't receive the infos block message"),
                                }
                                assert!(high_priority);
                                waitpoint_trigger_handle.trigger();
                                Ok(())
                            });
                    }
                });
            }
            TestsStepMatch::BlockManaged((asked_block_id, has_ops)) => {
                if has_ops {
                    foreign_controllers
                        .pool_controller
                        .set_expectations(|pool_controller| {
                            pool_controller
                                .expect_add_operations()
                                .times(1)
                                .in_sequence(&mut sequence)
                                .returning(move |_| {});
                        });
                }
                foreign_controllers
                    .consensus_controller
                    .expect_register_block()
                    .times(1)
                    .in_sequence(&mut sequence)
                    .return_once(move |block_id, _, _, _| {
                        assert_eq!(block_id, asked_block_id);
                        waitpoint_trigger_handle.trigger();
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
fn test_full_ask_block_workflow() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(100),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let op_1 = ProtocolTestUniverse::create_operation(&block_creator, 5);
    let op_thread = op_1
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, op_thread),
        Some(vec![op_1.clone()]),
        None,
    );
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    block_retrieval_mock(
        vec![
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_a_peer_id),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                AskForBlockInfo::Operations(
                    vec![op_1.id]
                        .into_iter()
                        .collect::<PreHashSet<OperationId>>()
                        .into_iter()
                        .collect(),
                ),
            )),
            TestsStepMatch::BlockManaged((block.id, true)),
        ],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
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
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![op_1.id]),
        })),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::Operations(vec![op_1]),
        })),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_empty_block() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(100),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    block_retrieval_mock(
        vec![
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_a_peer_id),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::BlockManaged((block.id, false)),
        ],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
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
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![]),
        })),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_dont_want_it_anymore() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(200),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let op_1 = ProtocolTestUniverse::create_operation(&block_creator, 5);
    let op_2 = ProtocolTestUniverse::create_operation(&block_creator, 5);
    let op_thread = op_1
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, op_thread),
        Some(vec![op_1.clone(), op_2.clone()]),
        None,
    );
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    block_retrieval_mock(
        vec![TestsStepMatch::AskData((
            PeerIdMatchers::PeerId(node_a_peer_id),
            block.id,
            AskForBlockInfo::OperationIds,
        ))],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
    );

    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config.clone());

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

    universe
        .module_controller
        .send_wishlist_delta(Default::default(), vec![block.id].into_iter().collect())
        .unwrap();

    std::thread::sleep(protocol_config.ask_block_timeout.to_duration());

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![op_1.id, op_2.id]),
        })),
    );

    // TODO: Find a better way to be sure that we don't asked the ops. For now we are just sleeping
    // and so if it's called during this timing the counts checks of mockall will panic.
    std::thread::sleep(
        protocol_config
            .ask_block_timeout
            .to_duration()
            .checked_mul(2)
            .unwrap(),
    );

    universe.stop();
}

#[test]
fn test_no_one_has_it() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(200),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    let mut peer_ids: HashSet<PeerId> = HashSet::new();
    peer_ids.insert(node_a_peer_id);
    peer_ids.insert(node_b_peer_id);
    block_retrieval_mock(
        vec![
            TestsStepMatch::AskData((
                PeerIdMatchers::AmongPeerIds(peer_ids.clone()),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskData((
                PeerIdMatchers::AmongPeerIds(peer_ids),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
        ],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
    );

    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config.clone());

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
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::NotFound,
        })),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_multiple_blocks_without_a_priori() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(100),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let block_1 = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 0), None, None);
    let block_2 = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .returning(move |_, _| {});
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        [node_a_peer_id, node_b_peer_id].iter().cloned().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
        let mut peer_ids = HashSet::new();
        peer_ids.insert(node_a_peer_id);
        peer_ids.insert(node_b_peer_id);
        let mut block_ids = HashSet::new();
        block_ids.insert(block_1.id);
        block_ids.insert(block_2.id);
        active_connections
            .expect_send_to_peer()
            .times(2..)
            .returning(move |peer_id, _, message, high_priority| {
                assert!(high_priority);
                match message {
                    Message::Block(message) => match *message {
                        BlockMessage::DataRequest { block_id, .. } => {
                            assert!(block_ids.contains(&block_id));
                        }
                        _ => panic!("Node didn't receive the infos block message"),
                    },
                    _ => panic!("Node didn't receive the infos block message"),
                }
                assert!(peer_ids.contains(peer_id));
                waitpoint_trigger_handle.trigger();
                Ok(())
            });
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config.clone());

    universe
        .module_controller
        .send_wishlist_delta(
            vec![
                (block_1.id, Some(block_1.content.header.clone())),
                (block_2.id, Some(block_2.content.header.clone())),
            ]
            .into_iter()
            .collect(),
            PreHashSet::<BlockId>::default(),
        )
        .unwrap();
    waitpoint.wait();
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_protocol_sends_blocks_when_asked_for() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(200),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);

    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    let peer_ids: HashSet<PeerId> = vec![node_a_peer_id, node_b_peer_id].into_iter().collect();
    block_retrieval_mock(
        vec![
            TestsStepMatch::SendHeader((
                PeerIdMatchers::AmongPeerIds(peer_ids.clone()),
                block.id,
                block.content.header.clone(),
            )),
            TestsStepMatch::SendHeader((
                PeerIdMatchers::AmongPeerIds(peer_ids),
                block.id,
                block.content.header.clone(),
            )),
            TestsStepMatch::SendData((
                PeerIdMatchers::PeerId(node_a_peer_id),
                block.id,
                BlockInfoReply::OperationIds(vec![]),
            )),
            TestsStepMatch::SendData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                BlockInfoReply::OperationIds(vec![]),
            )),
        ],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
    );

    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config.clone());

    universe.storage.store_block(block.clone());
    universe
        .module_controller
        .integrated_block(block.id, universe.storage.clone())
        .unwrap();
    waitpoint.wait();
    waitpoint.wait();

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::DataRequest {
            block_id: block.id,
            block_info: AskForBlockInfo::OperationIds,
        })),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataRequest {
            block_id: block.id,
            block_info: AskForBlockInfo::OperationIds,
        })),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_protocol_propagates_block_to_node_who_asked_for_operations_and_only_header_to_others() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ask_block_timeout: MassaTime::from_millis(200),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);

    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let node_c_keypair = KeyPair::generate(0).unwrap();
    let node_c_peer_id = PeerId::from_public_key(node_c_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let waipoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
            waipoint_trigger_handle.trigger();
        });
    let peer_ids: HashSet<PeerId> = vec![node_b_peer_id, node_c_peer_id].into_iter().collect();
    block_retrieval_mock(
        vec![
            TestsStepMatch::SendHeader((
                PeerIdMatchers::AmongPeerIds(peer_ids.clone()),
                block.id,
                block.content.header.clone(),
            )),
            TestsStepMatch::SendHeader((
                PeerIdMatchers::AmongPeerIds(peer_ids),
                block.id,
                block.content.header.clone(),
            )),
            TestsStepMatch::SendData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                BlockInfoReply::OperationIds(vec![]),
            )),
        ],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
    );

    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config.clone());

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );
    waitpoint.wait();

    universe.storage.store_block(block.clone());
    universe
        .module_controller
        .integrated_block(block.id, universe.storage.clone())
        .unwrap();
    waitpoint.wait();
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataRequest {
            block_id: block.id,
            block_info: AskForBlockInfo::OperationIds,
        })),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_noting_block_does_not_panic_with_one_max_node_known_blocks_size() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        max_known_blocks_size: 1,
        ask_block_timeout: MassaTime::from_millis(100),
        ..Default::default()
    };

    let block_creator = KeyPair::generate(0).unwrap();
    let op_1 = ProtocolTestUniverse::create_operation(&block_creator, 5);
    let op_thread = op_1
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, op_thread),
        Some(vec![op_1.clone()]),
        None,
    );
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    block_retrieval_mock(
        vec![
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_a_peer_id),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                AskForBlockInfo::OperationIds,
            )),
            TestsStepMatch::AskData((
                PeerIdMatchers::PeerId(node_b_peer_id),
                block.id,
                AskForBlockInfo::Operations(
                    vec![op_1.id]
                        .into_iter()
                        .collect::<PreHashSet<OperationId>>()
                        .into_iter()
                        .collect(),
                ),
            )),
            TestsStepMatch::BlockManaged((block.id, true)),
        ],
        &mut foreign_controllers,
        waitpoint.get_trigger_handle(),
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
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![op_1.id]),
        })),
    );
    waitpoint.wait();

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::Operations(vec![op_1]),
        })),
    );
    waitpoint.wait();

    universe.stop();
}
