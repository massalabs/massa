// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use massa_models::{block_id::BlockId, prehash::PreHashSet, slot::Slot};
use massa_protocol_exports::PeerId;
use massa_protocol_exports::{test_exports::tools, ProtocolConfig};
use massa_signature::KeyPair;
use massa_test_framework::{TestUniverse, WaitPoint};
use massa_time::MassaTime;
use mockall::predicate;
use parking_lot::{RwLock, RwLockWriteGuard};

use crate::handlers::peer_handler::models::{PeerInfo, PeerState};
use crate::wrap_network::{MockActiveConnectionsTrait, MockActiveConnectionsTraitWrapper};
use crate::wrap_peer_db::MockPeerDBTrait;
use crate::{
    handlers::{
        block_handler::{BlockInfoReply, BlockMessage},
        operation_handler::OperationMessage,
    },
    messages::Message,
};

use super::universe::{ProtocolForeignControllers, ProtocolTestUniverse};

fn peer_db_boilerplate(mock_peer_db: &mut RwLockWriteGuard<MockPeerDBTrait>) {
    mock_peer_db
        .expect_get_peers_in_test()
        .return_const(HashSet::default());
    mock_peer_db.expect_get_oldest_peer().return_const(None);
    mock_peer_db
        .expect_get_rand_peers_to_send()
        .return_const(vec![]);
}

#[test]
fn test_protocol_bans_node_sending_block_header_with_invalid_signature() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        unban_everyone_timer: MassaTime::from_millis(1000),
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);
    let mut block_bad_public_key = block.clone();
    block_bad_public_key.content.header.content_creator_pub_key =
        KeyPair::generate(0).unwrap().get_public_key();
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();
    let unban_waitpoint = WaitPoint::new();
    let unban_waitpoint_trigger_handle = unban_waitpoint.get_trigger_handle();

    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers_mut()
        .times(0..1)
        .returning(move || {
            let mut peers = HashMap::new();
            peers.insert(
                node_a_peer_id,
                PeerInfo {
                    last_announce: None,
                    state: PeerState::Trusted,
                },
            );
            peers
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            ban_waitpoint_trigger_handle.trigger();
        });
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .peer_db
        .write()
        .expect_unban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            unban_waitpoint_trigger_handle.trigger();
        });
    let mut peers = HashMap::new();
    peers.insert(
        node_a_peer_id,
        PeerInfo {
            last_announce: None,
            state: PeerState::Banned,
        },
    );
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(peers);
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    shared_active_connections.set_expectations(|active_connections| {
        active_connections
            .expect_get_peer_ids_connected()
            .returning(move || {
                let mut peers = HashSet::new();
                peers.insert(node_a_peer_id);
                peers
            });
        active_connections
            .expect_shutdown_connection()
            .times(1)
            .with(predicate::eq(node_a_peer_id))
            .returning(move |_| {});
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(
            block_bad_public_key.content.header.clone(),
        ))),
    );
    ban_waitpoint.wait();

    // After `unban_everyone_timer` the node should be unbanned
    unban_waitpoint.wait();
}

#[test]
fn test_protocol_bans_node_sending_operation_with_invalid_signature() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let op_creator = KeyPair::generate(0).unwrap();
    let mut operation = tools::create_operation_with_expire_period(&op_creator, 1);
    operation.content_creator_pub_key = KeyPair::generate(0).unwrap().get_public_key();
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();

    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers_mut()
        .times(0..1)
        .returning(move || {
            let mut peers = HashMap::new();
            peers.insert(
                node_a_peer_id,
                PeerInfo {
                    last_announce: None,
                    state: PeerState::Trusted,
                },
            );
            peers
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            ban_waitpoint_trigger_handle.trigger();
        });
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    shared_active_connections.set_expectations(|active_connections| {
        active_connections
            .expect_get_peer_ids_connected()
            .returning(move || {
                let mut peers = HashSet::new();
                peers.insert(node_a_peer_id);
                peers
            });
        active_connections
            .expect_shutdown_connection()
            .times(1)
            .with(predicate::eq(node_a_peer_id))
            .returning(move |_| {});
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![operation])),
    );
    ban_waitpoint.wait();
}

#[test]
fn test_protocol_bans_node_sending_header_with_invalid_signature() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, 1),
        Some(vec![operation_1]),
        None,
    );
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();

    let send_message_waitpoint = WaitPoint::new();
    let send_message_waitpoint_trigger_handle = send_message_waitpoint.get_trigger_handle();

    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers_mut()
        .times(0..1)
        .returning(move || {
            let mut peers = HashMap::new();
            peers.insert(
                node_a_peer_id,
                PeerInfo {
                    last_announce: None,
                    state: PeerState::Trusted,
                },
            );
            peers
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            ban_waitpoint_trigger_handle.trigger();
        });
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut peers = HashMap::new();
    peers.insert(
        node_a_peer_id,
        PeerInfo {
            last_announce: None,
            state: PeerState::Banned,
        },
    );
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(peers);
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    shared_active_connections.set_expectations(
        |active_connections: &mut MockActiveConnectionsTrait| {
            active_connections
                .expect_get_peer_ids_connected()
                .returning(move || {
                    let mut peers = HashSet::new();
                    peers.insert(node_a_peer_id);
                    peers
                });
            active_connections
                .expect_shutdown_connection()
                .times(1)
                .with(predicate::eq(node_a_peer_id))
                .returning(move |_| {});

            active_connections.expect_send_to_peer().times(1).returning(
                move |peer_id, _, _, high_priority| {
                    assert_eq!(peer_id, &node_a_peer_id);
                    //TODO: Add check messages
                    assert!(high_priority);
                    send_message_waitpoint_trigger_handle.trigger();
                    Ok(())
                },
            );
        },
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

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
    send_message_waitpoint.wait();

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![operation_2.id]),
        })),
    );
    ban_waitpoint.wait();
}

#[test]
fn test_protocol_does_not_asks_for_block_from_banned_node_who_propagated_header() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);
    let mut bad_block = block.clone();
    bad_block.content.header.content_creator_pub_key =
        KeyPair::generate(0).unwrap().get_public_key();
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();

    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers_mut()
        .times(0..1)
        .returning(move || {
            let mut peers = HashMap::new();
            peers.insert(
                node_a_peer_id,
                PeerInfo {
                    last_announce: None,
                    state: PeerState::Trusted,
                },
            );
            peers
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .times(1)
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            ban_waitpoint_trigger_handle.trigger();
        });
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut peers = HashMap::new();
    peers.insert(
        node_a_peer_id,
        PeerInfo {
            last_announce: None,
            state: PeerState::Banned,
        },
    );
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(peers);
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    shared_active_connections.set_expectations(
        |active_connections: &mut MockActiveConnectionsTrait| {
            active_connections
                .expect_get_peer_ids_connected()
                .returning(HashSet::new);
            active_connections
                .expect_shutdown_connection()
                .times(1)
                .with(predicate::eq(node_a_peer_id))
                .returning(move |_| {});
        },
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(
            bad_block.content.header.clone(),
        ))),
    );

    ban_waitpoint.wait();

    universe
        .module_controller
        .send_wishlist_delta(
            vec![(block.id, Some(block.content.header.clone()))]
                .into_iter()
                .collect(),
            PreHashSet::<BlockId>::default(),
        )
        .unwrap();

    //TODO: Find a way to check that no message will never be sent
    std::thread::sleep(Duration::from_millis(1000));
}

#[test]
fn test_protocol_bans_all_nodes_propagating_an_attack_attempt() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let block = ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), None, None);
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();
    let ban_waitpoint_trigger_handle_2 = ban_waitpoint.get_trigger_handle();

    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers_mut()
        .times(0..1)
        .returning(move || {
            let mut peers = HashMap::new();
            peers.insert(
                node_a_peer_id,
                PeerInfo {
                    last_announce: None,
                    state: PeerState::Trusted,
                },
            );
            peers.insert(
                node_b_peer_id,
                PeerInfo {
                    last_announce: None,
                    state: PeerState::Trusted,
                },
            );
            peers
        });
    let mut peers = HashMap::new();
    peers.insert(
        node_a_peer_id,
        PeerInfo {
            last_announce: None,
            state: PeerState::Banned,
        },
    );
    peers.insert(
        node_b_peer_id,
        PeerInfo {
            last_announce: None,
            state: PeerState::Banned,
        },
    );
    let counter = Arc::new(RwLock::new(0));
    let counter_clone = counter.clone();
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .with(predicate::eq(node_a_peer_id))
        .times(1)
        .returning(move |_| {
            let mut counter = counter.write();
            *counter += 1;
            if *counter == 2 {
                ban_waitpoint_trigger_handle.trigger();
            }
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .with(predicate::eq(node_b_peer_id))
        .times(1)
        .returning(move |_| {
            let mut counter = counter_clone.write();
            *counter += 1;
            if *counter == 2 {
                ban_waitpoint_trigger_handle_2.trigger();
            }
        });
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(peers);
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block.id);
            assert_eq!(header.id, block.content.header.id);
        });
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    shared_active_connections.set_expectations(
        |active_connections: &mut MockActiveConnectionsTrait| {
            active_connections
                .expect_get_peer_ids_connected()
                .returning(move || {
                    let mut peers = HashSet::new();
                    peers.insert(node_a_peer_id);
                    peers.insert(node_b_peer_id);
                    peers
                });
            active_connections
                .expect_shutdown_connection()
                .times(1)
                .with(predicate::eq(node_a_peer_id))
                .returning(move |_| {});
            active_connections
                .expect_shutdown_connection()
                .times(1)
                .with(predicate::eq(node_b_peer_id))
                .returning(move |_| {});
        },
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );

    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );

    // TODO: Find a way to wait for both previous messages to be processed because it doesn't call any mock for second block same as first
    std::thread::sleep(Duration::from_millis(1000));

    universe
        .module_controller
        .notify_block_attack(block.id)
        .unwrap();

    ban_waitpoint.wait();
}
