use massa_models::slot::Slot;
use massa_pos_exports::Selection;
use massa_protocol_exports::PeerId;
use massa_protocol_exports::ProtocolConfig;
use massa_signature::KeyPair;
use massa_test_framework::{TestUniverse, WaitPoint};

use crate::{
    handlers::{block_handler::BlockMessage, endorsement_handler::EndorsementMessage},
    messages::Message,
    wrap_network::MockActiveConnectionsTraitWrapper,
};

use super::universe::{ProtocolForeignControllers, ProtocolTestUniverse};

#[test]
fn test_protocol_sends_valid_endorsements_it_receives_to_pool() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let peer_ids = [node_a_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&endorsement.id));
                    waitpoint_trigger_handle.trigger();
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement.clone()])),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_protocol_does_not_send_invalid_endorsements_it_receives_to_pool() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let peer_ids = [node_a_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let mut endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));
    endorsement.content_creator_pub_key = node_a_keypair.get_public_key();

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections
            .expect_shutdown_connection()
            .returning(move |_| ());
    });
    foreign_controllers
        .peer_db
        .write()
        .expect_ban_peer()
        .returning(move |peer_id| {
            assert_eq!(peer_id, &node_a_peer_id);
            waitpoint_trigger_handle.trigger();
        });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller.expect_add_endorsements().never();
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement.clone()])),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_protocol_propagates_endorsements_to_active_nodes() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let peer_ids = [node_a_peer_id, node_b_peer_id];
    let endorsement_creator = KeyPair::generate(0).unwrap();
    let endorsement =
        ProtocolTestUniverse::create_endorsement(&endorsement_creator, Slot::new(1, 1));
    let endorsement_clone = endorsement.clone();

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections.expect_send_to_peer().returning(
            move |peer_id, _message_serializer, message, _high_priority| {
                assert_eq!(peer_id, &node_b_peer_id);
                match message {
                    Message::Endorsement(EndorsementMessage::Endorsements(endorsements)) => {
                        assert_eq!(endorsements.len(), 1);
                        assert_eq!(endorsements[0], endorsement_clone);
                        waitpoint_trigger_handle.trigger();
                    }
                    _ => panic!("Unexpected message type"),
                }
                Ok(())
            },
        );
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&endorsement.id));
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Endorsement(EndorsementMessage::Endorsements(vec![endorsement.clone()])),
    );
    waitpoint.wait();

    universe.stop();
}

#[test]
fn test_protocol_propagates_endorsements_only_to_nodes_that_dont_know_about_it_block_integration() {
    let protocol_config = ProtocolConfig {
        thread_count: 2,
        ..Default::default()
    };
    let node_a_keypair = KeyPair::generate(0).unwrap();
    let node_a_peer_id = PeerId::from_public_key(node_a_keypair.get_public_key());
    let node_b_keypair = KeyPair::generate(0).unwrap();
    let node_b_peer_id = PeerId::from_public_key(node_b_keypair.get_public_key());
    let peer_ids = [node_a_peer_id, node_b_peer_id];
    let block_creator = KeyPair::generate(0).unwrap();
    let endorsement = ProtocolTestUniverse::create_endorsement(&block_creator, Slot::new(1, 1));
    let endorsement_clone = endorsement.clone();
    let block = ProtocolTestUniverse::create_block(
        &block_creator,
        Slot::new(1, 1),
        None,
        Some(vec![endorsement.clone()]),
    );

    let waitpoint = WaitPoint::new();
    let waitpoint_trigger_handle = waitpoint.get_trigger_handle();
    let waitpoint_trigger_handle2 = waitpoint.get_trigger_handle();
    let mut foreign_controllers = ProtocolForeignControllers::new_with_mocks();
    ProtocolTestUniverse::peer_db_boilerplate(&mut foreign_controllers.peer_db.write());
    let mut shared_active_connections = MockActiveConnectionsTraitWrapper::new();
    ProtocolTestUniverse::active_connections_boilerplate(
        &mut shared_active_connections,
        peer_ids.into_iter().collect(),
    );
    shared_active_connections.set_expectations(|active_connections| {
        active_connections.expect_send_to_peer().times(1).returning(
            move |peer_id, _message_serializer, message, _high_priority| {
                assert_eq!(peer_id, &node_b_peer_id);
                println!("message: {:?}", message);
                match message {
                    Message::Endorsement(EndorsementMessage::Endorsements(endorsements)) => {
                        assert_eq!(endorsements.len(), 1);
                        assert_eq!(endorsements[0], endorsement_clone);
                        waitpoint_trigger_handle2.trigger();
                    }
                    _ => panic!("Unexpected message type"),
                }
                Ok(())
            },
        );
    });
    foreign_controllers
        .network_controller
        .expect_get_active_connections()
        .returning(move || Box::new(shared_active_connections.clone()));
    foreign_controllers
        .pool_controller
        .set_expectations(|pool_controller| {
            pool_controller
                .expect_add_endorsements()
                .return_once(move |endorsements_storage| {
                    let stored_endorsements = endorsements_storage.get_endorsement_refs();
                    assert_eq!(stored_endorsements.len(), 1);
                    assert!(stored_endorsements.contains(&endorsement.id));
                });
        });
    foreign_controllers
        .selector_controller
        .set_expectations(|selector_controller| {
            selector_controller
                .expect_get_selection()
                .return_once(move |slot| {
                    assert_eq!(slot, endorsement.content.slot);
                    Ok(Selection {
                        endorsements: vec![endorsement.content_creator_address; 1],
                        producer: endorsement.content_creator_address,
                    })
                });
        });
    foreign_controllers
        .consensus_controller
        .expect_register_block_header()
        .return_once(move |block_id, block| {
            assert_eq!(block_id, block.id);
            waitpoint_trigger_handle.trigger();
        });
    let mut universe = ProtocolTestUniverse::new(foreign_controllers, protocol_config);

    universe.mock_message_receive(
        &node_a_peer_id,
        Message::Block(Box::new(BlockMessage::Header(block.content.header))),
    );
    waitpoint.wait();

    // universe.storage.store_endorsements(vec![endorsement]);
    // universe
    //     .module_controller
    //     .propagate_endorsements(universe.storage.clone())
    //     .unwrap();
    waitpoint.wait();

    universe.stop();
}
