// Copyright (c) 2022 MASSA LABS <info@massa.net>

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use massa_models::config::CHAINID;
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
        endorsement_handler::EndorsementMessage,
        operation_handler::OperationMessage,
        peer_handler::PeerManagementMessage,
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
    let block =
        ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), vec![], vec![], vec![]);
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
    let operation_1 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, 1),
        vec![operation_1],
        vec![],
        vec![],
    );
    let operation_2 = ProtocolTestUniverse::create_operation(&block_creator, 1, *CHAINID);
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
    let block =
        ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), vec![], vec![], vec![]);
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

/// A peer that never sent us any data for a block must not be banned when that block
/// turns out to be an attack attempt, even though our propagation cache marks it as
/// knowing the block (because we announced the header to it ourselves).
#[test]
fn test_protocol_only_bans_the_senders_of_an_attack_block() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let block =
        ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), vec![], vec![], vec![]);
    // node A sends us the block header, node B only receives our announcement of it
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

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
    let mut peers = HashMap::new();
    peers.insert(
        node_a_peer_id,
        PeerInfo {
            last_announce: None,
            state: PeerState::Banned,
        },
    );
    // only the peer that actually sent us the block gets banned
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .with(predicate::eq(node_a_peer_id))
        .times(1)
        .returning(move |_| {
            ban_waitpoint_trigger_handle.trigger();
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .with(predicate::eq(node_b_peer_id))
        .times(0)
        .returning(move |_| {});
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(peers);
    let block_clone = block.clone();
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block_clone.id);
            assert_eq!(header.id, block_clone.content.header.id);
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
            // we announce the header to node B, which marks it as knowing the block
            active_connections
                .expect_send_to_peer()
                .returning(move |_, _, _, _| Ok(()));
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

    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    // node A sends us the header: it is recorded as a direct sender of that block
    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );

    // we announce the header to our peers, marking them as knowing the block
    universe.storage.store_block(block.clone());
    universe
        .module_controller
        .integrated_block(block.id, universe.storage.clone())
        .unwrap();

    // TODO: Find a way to wait for the previous messages to be processed
    std::thread::sleep(Duration::from_millis(1000));

    universe
        .module_controller
        .notify_block_attack(block.id)
        .unwrap();

    ban_waitpoint.wait();
}

/// A block whose contents turn out to be invalid must only get the peers that actually sent us
/// those contents banned. A peer that merely relayed the header had no way of checking the
/// contents before propagating, so it must be spared.
#[test]
fn test_protocol_does_not_ban_header_only_sender_of_an_invalid_block() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        // any single operation is enough to overflow the block, making its contents invalid
        max_serialized_operations_size_per_block: 1,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let op = tools::create_operation_with_expire_period(&block_creator, 5);
    let op_thread = op
        .content_creator_address
        .get_thread(protocol_config.thread_count);
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, op_thread),
        vec![op.clone()],
        vec![],
        vec![],
    );
    // node A only relays the header, node B sends us the operation ids
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();

    // only the peer that sent us the block contents gets banned
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .with(predicate::eq(node_b_peer_id))
        .times(1)
        .returning(move |_| {
            ban_waitpoint_trigger_handle.trigger();
        });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .with(predicate::eq(node_a_peer_id))
        .times(0)
        .returning(move |_| {});
    peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(HashMap::default());
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers_mut()
        .times(0..1)
        .returning(HashMap::default);
    let block_clone = block.clone();
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block_clone.id);
            assert_eq!(header.id, block_clone.content.header.id);
        });
    let block_clone = block.clone();
    foreign_controllers
        .consensus_controller
        .expect_mark_invalid_block()
        .return_once(move |block_id, header| {
            assert_eq!(block_id, block_clone.id);
            assert_eq!(header.id, block_clone.content.header.id);
        });
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    shared_active_connections.set_expectations(
        |active_connections: &mut MockActiveConnectionsTrait| {
            // only node B is connected, so it is the one we ask the block contents from
            active_connections
                .expect_get_peer_ids_connected()
                .returning(move || {
                    let mut peers = HashSet::new();
                    peers.insert(node_b_peer_id);
                    peers
                });
            active_connections
                .expect_send_to_peer()
                .returning(move |_, _, _, _| Ok(()));
            active_connections
                .expect_shutdown_connection()
                .with(predicate::eq(node_b_peer_id))
                .returning(move |_| {});
        },
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));

    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    // make the operation available locally so that the block size check can be performed
    universe.storage.store_operations(vec![op.clone()]);

    // node A relays the header: it is recorded as a header-only sender of that block
    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );

    // start retrieving the block, which makes us ask node B for its operation ids
    universe
        .module_controller
        .send_wishlist_delta(
            vec![(block.id, Some(block.content.header.clone()))]
                .into_iter()
                .collect(),
            PreHashSet::<BlockId>::default(),
        )
        .unwrap();

    // TODO: Find a way to wait for the previous messages to be processed
    std::thread::sleep(Duration::from_millis(1000));

    // node B sends us the contents: the block is then found to be oversized and invalid
    universe.mock_message_receive(
        &node_b_peer_id,
        Message::Block(Box::new(BlockMessage::DataResponse {
            block_id: block.id,
            block_info: BlockInfoReply::OperationIds(vec![op.id]),
        })),
    );

    ban_waitpoint.wait();
}

#[test]
fn test_protocol_bans_all_nodes_propagating_an_attack_attempt() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };

    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();

    let block_creator = KeyPair::generate(0).unwrap();
    let block =
        ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), vec![], vec![], vec![]);
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

/// Common mock setup for the trailing-bytes ban tests: the peer must be
/// disconnected and banned, and nothing else must happen.
fn trailing_bytes_ban_setup(
    foreign_controllers: &mut ProtocolForeignControllers,
    node_a_peer_id: PeerId,
    ban_waitpoint: &WaitPoint,
) {
    let ban_waitpoint_trigger_handle = ban_waitpoint.get_trigger_handle();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            ban_waitpoint_trigger_handle.trigger();
        });
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        [node_a_peer_id].into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections
            .expect_shutdown_connection()
            .returning(move |peer_id| {
                assert_eq!(peer_id, &node_a_peer_id);
            });
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
}

#[test]
fn test_protocol_bans_node_sending_block_message_with_trailing_bytes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let block_creator = KeyPair::generate(0).unwrap();
    let block =
        ProtocolTestUniverse::create_block(&block_creator, Slot::new(1, 1), vec![], vec![], vec![]);

    let ban_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    trailing_bytes_ban_setup(&mut foreign_controllers, node_a_peer_id, &ban_waitpoint);
    // the message must be dropped, not processed
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .never();

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive_with_trailing_bytes(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header.clone()))),
    );
    ban_waitpoint.wait();
}

#[test]
fn test_protocol_bans_node_sending_operation_message_with_trailing_bytes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let op_creator = KeyPair::generate(0).unwrap();
    let operation = tools::create_operation_with_expire_period(&op_creator, 1);

    let ban_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    trailing_bytes_ban_setup(&mut foreign_controllers, node_a_peer_id, &ban_waitpoint);
    // the message must be dropped, not processed
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller.expect_add_operations().never();
        });

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive_with_trailing_bytes(
        &node_a_peer_id,
        Message::Operation(OperationMessage::Operations(vec![operation])),
    );
    ban_waitpoint.wait();
}

#[test]
fn test_protocol_bans_node_sending_endorsement_message_with_trailing_bytes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));

    let ban_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    trailing_bytes_ban_setup(&mut foreign_controllers, node_a_peer_id, &ban_waitpoint);
    // the message must be dropped, not processed
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller.expect_add_endorsements().never();
        });

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive_with_trailing_bytes(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement])),
    );
    ban_waitpoint.wait();
}

#[test]
fn test_protocol_bans_node_sending_peer_management_message_with_trailing_bytes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());

    let ban_waitpoint = WaitPoint::new();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    trailing_bytes_ban_setup(&mut foreign_controllers, node_a_peer_id, &ban_waitpoint);
    // banned-state lookup done by the peer handler before deserializing
    foreign_controllers
        .peer_db
        .write()
        .expect_get_peers()
        .return_const(HashMap::new());

    let universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive_with_trailing_bytes(
        &node_a_peer_id,
        Message::PeerManagement(Box::new(PeerManagementMessage::ListPeers(vec![]))),
    );
    ban_waitpoint.wait();
}
